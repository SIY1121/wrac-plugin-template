use std::ffi::{c_char, c_void};
use std::ptr;

use clap_sys::factory::plugin_factory::clap_plugin_factory;

use crate::descriptor::ClapDescriptorStorage;
use crate::entry::EntryRegistration;

pub(crate) struct PluginRegistrationStorage {
    pub clap_factory: ClapFactoryState,
    pub auv2_factory: Auv2FactoryState,
    pub descriptors: Vec<ClapDescriptorStorage>,
}

// Safety: after creation the storage only reads out factory/descriptor pointers.
// Internal pointers point to buffers owned by this same storage, and `OnceLock`
// prevents initialization races.
unsafe impl Sync for PluginRegistrationStorage {}
unsafe impl Send for PluginRegistrationStorage {}

impl PluginRegistrationStorage {
    pub(crate) fn new(registration: &'static EntryRegistration) -> Self {
        let descriptors = registration
            .entry
            .plugin_factory()
            .map(|factory| {
                (0..factory.plugin_count())
                    .filter_map(|index| factory.plugin_descriptor(index))
                    .map(ClapDescriptorStorage::new)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Self {
            clap_factory: ClapFactoryState {
                factory: clap_plugin_factory {
                    get_plugin_count: Some(crate::abi::factory_get_plugin_count),
                    get_plugin_descriptor: Some(crate::abi::factory_get_plugin_descriptor),
                    create_plugin: Some(crate::abi::factory_create_plugin),
                },
                registration,
            },
            auv2_factory: Auv2FactoryState {
                factory: ClapPluginFactoryAsAuv2 {
                    manufacturer_code: descriptors
                        .iter()
                        .find_map(ClapDescriptorStorage::auv2_manufacturer_code_ptr)
                        .unwrap_or(ptr::null()),
                    manufacturer_name: descriptors
                        .iter()
                        .find_map(ClapDescriptorStorage::auv2_manufacturer_name_ptr)
                        .unwrap_or(ptr::null()),
                    get_auv2_info: Some(crate::abi::auv2_get_info),
                },
                registration,
            },
            descriptors,
        }
    }
}

// CLAP factory callbacks receive only a factory pointer, so the C ABI struct is placed
// as the first field and cast back to the state inside the callback.
#[repr(C)]
pub(crate) struct ClapFactoryState {
    pub factory: clap_plugin_factory,
    pub registration: &'static EntryRegistration,
}

unsafe impl Sync for ClapFactoryState {}
unsafe impl Send for ClapFactoryState {}

#[repr(C)]
pub(crate) struct Auv2FactoryState {
    pub factory: ClapPluginFactoryAsAuv2,
    pub registration: &'static EntryRegistration,
}

unsafe impl Sync for Auv2FactoryState {}
unsafe impl Send for Auv2FactoryState {}

#[repr(C)]
pub(crate) struct ClapPluginInfoAsAuv2 {
    pub au_type: [c_char; 5],
    pub au_subt: [c_char; 5],
}

#[repr(C)]
pub(crate) struct ClapPluginFactoryAsAuv2 {
    pub manufacturer_code: *const c_char,
    pub manufacturer_name: *const c_char,
    pub get_auv2_info: Option<
        unsafe extern "C" fn(
            factory: *const ClapPluginFactoryAsAuv2,
            index: u32,
            info: *mut ClapPluginInfoAsAuv2,
        ) -> bool,
    >,
}

unsafe impl Sync for ClapPluginFactoryAsAuv2 {}
unsafe impl Send for ClapPluginFactoryAsAuv2 {}

pub(crate) fn clap_factory_state(
    factory: *const clap_plugin_factory,
) -> Option<&'static ClapFactoryState> {
    if factory.is_null() {
        return None;
    }
    Some(unsafe { &*(factory as *const ClapFactoryState) })
}

pub(crate) fn auv2_factory_state(
    factory: *const ClapPluginFactoryAsAuv2,
) -> Option<&'static Auv2FactoryState> {
    if factory.is_null() {
        return None;
    }
    Some(unsafe { &*(factory as *const Auv2FactoryState) })
}

pub(crate) fn factory_ptr(storage: &'static PluginRegistrationStorage) -> *const c_void {
    &storage.clap_factory.factory as *const clap_plugin_factory as *const c_void
}

pub(crate) fn auv2_factory_ptr(storage: &'static PluginRegistrationStorage) -> *const c_void {
    &storage.auv2_factory.factory as *const ClapPluginFactoryAsAuv2 as *const c_void
}
