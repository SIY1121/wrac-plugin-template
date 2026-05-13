use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;
use wrac_clap_adapter::{
    ClapWindow, GuiApi, GuiConfiguration, GuiResizeHints, GuiSize, PluginError, PluginGui,
    PluginResult,
};

use crate::runtime::{
    GuiRuntimeHandle, WxpGuiFactory, create_gui_runtime_handle, is_gui_thread,
    release_gui_thread_ref,
};
use crate::window::StoredParentWindow;

#[derive(Debug, Clone, Copy)]
pub struct GuiSizeLimits {
    pub min: GuiSize,
    pub max: GuiSize,
}

/// wxp WebView runtime を `PluginGui` として公開する Send/Sync controller。
///
/// 実 runtime は UI thread の TLS 上に保持し、この型は `PluginInstance` から共有される
/// handle として GUI lifecycle callback を受ける。現在は host parent に child view として
/// 貼る embedded GUI のみ対応し、floating window は拒否する。
pub struct WxpGuiController {
    factory: Box<dyn WxpGuiFactory>,
    layout: HostGuiLayout,
    runtime: Mutex<GuiRuntimeState>,
}

struct HostGuiLayout {
    // CLAP layout queries read this without entering the GUI runtime.
    // The accepted size is the controller's host contract, not copied runtime state.
    accepted_size: AtomicGuiSize,
    limits: GuiSizeLimits,
    resize_policy: GuiResizePolicy,
}

struct GuiRuntimeState {
    scale: f64,
    parent: Option<StoredParentWindow>,
    handle: Option<GuiRuntimeHandle>,
}

impl WxpGuiController {
    pub fn new(factory: impl WxpGuiFactory, initial_size: GuiSize) -> Self {
        let size_limits = GuiSizeLimits {
            min: GuiSize {
                width: 1,
                height: 1,
            },
            max: GuiSize {
                width: u32::MAX,
                height: u32::MAX,
            },
        };
        Self {
            factory: Box::new(factory),
            layout: HostGuiLayout::new(initial_size, size_limits, GuiResizePolicy::RESIZABLE),
            runtime: Mutex::new(GuiRuntimeState {
                scale: 1.0,
                parent: None,
                handle: None,
            }),
        }
    }

    pub fn with_size_limits(mut self, limits: GuiSizeLimits) -> Self {
        self.layout.set_limits(limits);
        self
    }

    fn destroy_runtime_and_parent_ref(&self) {
        let (handle, release_parent_ref) = {
            let mut state = self.runtime.lock();
            let handle = state.handle.take();
            let release_parent_ref = state.parent.take().is_some();
            (handle, release_parent_ref)
        };
        if let Some(handle) = handle {
            handle.destroy();
        }
        if release_parent_ref {
            release_gui_thread_ref();
        }
    }

    fn create_runtime(
        &self,
        configuration: GuiConfiguration,
        size: GuiSize,
        parent: StoredParentWindow,
        scale: f64,
    ) -> PluginResult<GuiRuntimeHandle> {
        let parent = parent.to_parent_window_handle()?;
        let handle = create_gui_runtime_handle(|| {
            self.factory.create_gui_runtime(configuration, size, parent)
        })?;
        if let Err(error) = handle.set_scale(scale) {
            handle.destroy();
            return Err(error);
        }
        Ok(handle)
    }
}

impl HostGuiLayout {
    fn new(size: GuiSize, limits: GuiSizeLimits, resize_policy: GuiResizePolicy) -> Self {
        let size = clamp_size_with_limits(size, limits);
        Self {
            accepted_size: AtomicGuiSize::new(size),
            limits,
            resize_policy,
        }
    }

    fn accepted_size(&self) -> GuiSize {
        self.accepted_size.load()
    }

    fn set_limits(&mut self, limits: GuiSizeLimits) {
        self.limits = limits;
        self.accepted_size
            .store(self.clamp_size(self.accepted_size()));
    }

    fn clamp_size(&self, size: GuiSize) -> GuiSize {
        clamp_size_with_limits(size, self.limits)
    }

    fn store_accepted_size(&self, size: GuiSize) {
        self.accepted_size.store(size);
    }

    fn can_resize(&self) -> bool {
        self.resize_policy.can_resize()
    }

    fn resize_hints(&self) -> GuiResizeHints {
        self.resize_policy.resize_hints()
    }
}

struct AtomicGuiSize(AtomicU64);

impl AtomicGuiSize {
    fn new(size: GuiSize) -> Self {
        Self(AtomicU64::new(pack_size(size)))
    }

    fn load(&self) -> GuiSize {
        unpack_size(self.0.load(Ordering::Relaxed))
    }

    fn store(&self, size: GuiSize) {
        self.0.store(pack_size(size), Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy)]
struct GuiResizePolicy {
    can_resize: bool,
}

impl GuiResizePolicy {
    const RESIZABLE: Self = Self { can_resize: true };

    fn can_resize(self) -> bool {
        self.can_resize
    }

    fn resize_hints(self) -> GuiResizeHints {
        GuiResizeHints {
            can_resize_horizontally: self.can_resize,
            can_resize_vertically: self.can_resize,
            preserve_aspect_ratio: false,
            aspect_ratio_width: 0,
            aspect_ratio_height: 0,
        }
    }
}

fn pack_size(size: GuiSize) -> u64 {
    ((size.width as u64) << 32) | size.height as u64
}

fn unpack_size(size: u64) -> GuiSize {
    GuiSize {
        width: (size >> 32) as u32,
        height: size as u32,
    }
}

impl PluginGui for WxpGuiController {
    fn is_api_supported(&self, api: GuiApi, is_floating: bool) -> bool {
        !is_floating && api == default_gui_api()
    }

    fn preferred_api(&self) -> Option<GuiConfiguration> {
        Some(default_gui_configuration())
    }

    fn create(&self, configuration: GuiConfiguration) -> PluginResult<()> {
        if !self.is_api_supported(configuration.api, configuration.is_floating) {
            return Err(PluginError::Message("unsupported GUI configuration"));
        }
        Ok(())
    }

    fn destroy(&self) {
        self.destroy_runtime_and_parent_ref();
    }

    fn set_scale(&self, scale: f64) -> PluginResult<()> {
        let handle = { self.runtime.lock().handle.clone() };
        if let Some(handle) = handle {
            handle.set_scale(scale)?;
        }
        self.runtime.lock().scale = scale;
        Ok(())
    }

    fn get_size(&self) -> PluginResult<GuiSize> {
        Ok(self.layout.accepted_size())
    }

    fn can_resize(&self) -> bool {
        self.layout.can_resize()
    }

    fn resize_hints(&self) -> Option<GuiResizeHints> {
        Some(self.layout.resize_hints())
    }

    fn adjust_size(&self, size: GuiSize) -> PluginResult<GuiSize> {
        Ok(self.layout.clamp_size(size))
    }

    fn set_size(&self, size: GuiSize) -> PluginResult<()> {
        let size = self.layout.clamp_size(size);
        let handle = { self.runtime.lock().handle.clone() };
        if let Some(handle) = handle {
            handle.set_size(size)?;
        }
        self.layout.store_accepted_size(size);
        Ok(())
    }

    fn set_parent(&self, window: ClapWindow) -> PluginResult<()> {
        // parent window は host UI thread に属する。WebView の生成自体は `show()` まで
        // 遅らせるが、thread ownership は最初の `set_parent()` で固定しておく。
        {
            let state = self.runtime.lock();
            if state.parent.is_some() {
                if !is_gui_thread() {
                    return Err(PluginError::UnsupportedHostGuiThreadingModel);
                }
            } else {
                drop(state);
                crate::runtime::acquire_gui_thread()?;
            }
        }

        let parent = StoredParentWindow::from_clap_window(window);
        let old_handle = {
            let mut state = self.runtime.lock();
            state.parent = Some(parent);
            state.handle.take()
        };
        if let Some(handle) = old_handle {
            handle.destroy();
        }

        let create = {
            let state = self.runtime.lock();
            // CLAP の GUI spec 上は `show()` が表示開始の合図だが、clap-wrapper の AUv2
            // 経由では Logic Pro が embedded view を parent に付けた時点で表示扱いにし、
            // `show()` を呼ばない経路がある。WebView 作成を `show()` だけに遅延すると
            // host の空 container だけが表示されるため、parent が確定した時点で runtime を作る。
            state
                .handle
                .is_none()
                .then_some((self.layout.accepted_size(), parent, state.scale))
        };
        if let Some((size, parent, scale)) = create {
            let handle = self.create_runtime(default_gui_configuration(), size, parent, scale)?;
            self.runtime.lock().handle = Some(handle);
        }
        Ok(())
    }

    fn set_transient(&self, _window: ClapWindow) -> PluginResult<()> {
        Err(PluginError::Message("floating GUI is unsupported"))
    }

    fn suggest_title(&self, _title: &str) {}

    fn show(&self) -> PluginResult<()> {
        let action = {
            let state = self.runtime.lock();
            if let Some(handle) = state.handle.clone() {
                ShowAction::ShowExisting(handle)
            } else {
                let parent = state.parent.ok_or(PluginError::InvalidState)?;
                ShowAction::Create {
                    size: self.layout.accepted_size(),
                    parent,
                    scale: state.scale,
                }
            }
        };

        match action {
            ShowAction::ShowExisting(handle) => handle.show(),
            ShowAction::Create {
                size,
                parent,
                scale,
            } => {
                let handle =
                    self.create_runtime(default_gui_configuration(), size, parent, scale)?;
                handle.show()?;
                self.runtime.lock().handle = Some(handle);
                Ok(())
            }
        }
    }

    fn hide(&self) -> PluginResult<()> {
        let handle = {
            let mut state = self.runtime.lock();
            state.handle.take()
        };
        if let Some(handle) = handle {
            let _ = handle.hide();
            // wxp の public handle は visible 切替を直接公開していない。`hide()` では
            // runtime を破棄して、CLAP の「見えていない」状態を確実に満たす。
            handle.destroy();
        }
        Ok(())
    }
}

fn clamp_size_with_limits(size: GuiSize, limits: GuiSizeLimits) -> GuiSize {
    GuiSize {
        width: size.width.clamp(limits.min.width, limits.max.width),
        height: size.height.clamp(limits.min.height, limits.max.height),
    }
}

impl Drop for WxpGuiController {
    fn drop(&mut self) {
        self.destroy_runtime_and_parent_ref();
    }
}

enum ShowAction {
    ShowExisting(GuiRuntimeHandle),
    Create {
        size: GuiSize,
        parent: StoredParentWindow,
        scale: f64,
    },
}

fn default_gui_api() -> GuiApi {
    if cfg!(target_os = "macos") {
        GuiApi::Cocoa
    } else if cfg!(target_os = "windows") {
        GuiApi::Win32
    } else {
        GuiApi::X11
    }
}

fn default_gui_configuration() -> GuiConfiguration {
    GuiConfiguration {
        api: default_gui_api(),
        is_floating: false,
    }
}
