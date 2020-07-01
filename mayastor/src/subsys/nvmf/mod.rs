//!
//! The NVMF target implementation is used to export replicas
//! but also, if desired a nexus device. A target makes use of
//! several transports, what transports that exactly is -- is flexible.
//!
//! In our case we currently only deal with TCP. We create two transports
//! one for the frontend (nexus) and one for the backend (replica)
//!
//! As connections come on, we randomly schedule them across cores by putting
//! the qpair in a poll group that is allocated during reactor start.
use std::cell::RefCell;

use nix::errno::Errno;
use snafu::Snafu;

use poll_groups::PollGroup;
use spdk_sys::{
    spdk_subsystem,
    spdk_subsystem_fini_next,
    spdk_subsystem_init_next,
};
pub use subsystem::NvmfSubsystem;
pub use target::Target;

use crate::{core::Bdev, subsys::Config};

mod poll_groups;
mod subsystem;
mod target;
mod transport;

// wrapper around our NVMF subsystem used for registration
pub struct Nvmf(pub(crate) *mut spdk_subsystem);

impl Default for Nvmf {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to create nvmf target {}", msg))]
    CreateTarget { msg: String },
    #[snafu(display(
        "Failed to destroy nvmf target {}: {}",
        endpoint,
        source
    ))]
    DestroyTarget { source: Errno, endpoint: String },
    #[snafu(display("Failed to create poll groups {}", msg))]
    PgError { msg: String },
    #[snafu(display("Failed to create transport {}", msg))]
    Transport { source: Errno, msg: String },
    #[snafu(display("Failed to create subsystem for {} {} error: {}", source.desc(), nqn, msg))]
    Subsystem {
        source: Errno,
        nqn: String,
        msg: String,
    },
    #[snafu(display("Failed to create share for  {} {}", bdev, msg))]
    Share { bdev: Bdev, msg: String },
    #[snafu(display("Failed to add namespace for  {} {}", bdev, msg))]
    Namespace { bdev: Bdev, msg: String },
}

thread_local! {
    pub (crate) static NVMF_TGT: RefCell<Target> = RefCell::new(Target::new());
    pub (crate) static NVMF_PGS: RefCell<Vec<PollGroup>> = RefCell::new(Vec::new());
}

impl Nvmf {
    /// initialize a new subsystem that handles NVMF (confusing names, cannot
    /// help it)
    extern "C" fn init() {
        debug!("mayastor nvmf subsystem init");

        // this code only ever gets run on the fist core so we dont
        // end up running this on other cores.

        if Config::get().nexus_opts.nvmf_enable {
            NVMF_TGT.with(|tgt| {
                tgt.borrow_mut().next_state();
            });
        } else {
            debug!("nvmf targe disabled");
            unsafe { spdk_subsystem_init_next(0) }
        }
    }

    extern "C" fn fini() {
        debug!("mayastor nvmf fini");
        if Config::get().nexus_opts.nvmf_enable {
            NVMF_TGT.with(|tgt| {
                tgt.borrow_mut().start_shutdown();
            });
        } else {
            unsafe { spdk_subsystem_fini_next() }
        }
    }

    pub fn new() -> Self {
        debug!("creating Mayastor nvmf subsystem...");
        let mut ss = Box::new(spdk_subsystem::default());
        ss.name = b"mayastor_nvmf_tgt\x00" as *const u8 as *const libc::c_char;
        ss.init = Some(Self::init);
        ss.fini = Some(Self::fini);
        ss.write_config_json = None;
        Self(Box::into_raw(ss))
    }
}
