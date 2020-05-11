extern crate log;

use crossbeam::channel::unbounded;

use std::{ffi::CString, time::Duration};
pub mod common;
use mayastor::{
    bdev::{nexus_create, nexus_lookup, NexusState},
    core::{
        mayastor_env_stop,
        Bdev,
        MayastorCliArgs,
        MayastorEnvironment,
        Reactor,
    },
    subsys::Config,
};

use spdk_sys::{
    create_aio_bdev,
    spdk_vbdev_error_create,
    spdk_vbdev_error_inject_error,
    SPDK_BDEV_IO_TYPE_READ,
    SPDK_BDEV_IO_TYPE_WRITE,
};

static ERROR_COUNT_TEST_NEXUS: &str = "error_fault_child_test_nexus";

static DISKNAME1: &str = "/tmp/disk1.img";
static BDEVNAME1: &str = "aio:///tmp/disk1.img?blk_size=512";

static DISKNAME2: &str = "/tmp/disk2.img";

static ERROR_DEVICE: &str = "error_device";
static EE_ERROR_DEVICE: &str = "EE_error_device"; // The prefix is added by the vbdev_error module
static BDEV_EE_ERROR_DEVICE: &str = "bdev:///EE_error_device";

static YAML_CONFIG_FILE: &str = "/tmp/error_fault_child_test_nexus.yaml";

// constant used by the vbdev_error module but not exported
const VBDEV_IO_FAILURE: u32 = 1;

#[test]
fn nexus_fault_child_test() {
    common::truncate_file(DISKNAME1, 64 * 1024);
    common::truncate_file(DISKNAME2, 64 * 1024);

    let mut config = Config::default();
    config.err_monitoring_opts.enable_err_store = true;
    config.err_monitoring_opts.err_store_size = 256;
    config.err_monitoring_opts.fault_child_on_error = true;
    config.err_monitoring_opts.max_error_age_ns = 1_000_000_000;
    config.err_monitoring_opts.max_retry_errors = 4;

    config.write(YAML_CONFIG_FILE).unwrap();

    test_init!(Some(YAML_CONFIG_FILE.to_string()));

    Reactor::block_on(async {
        create_error_bdev().await;
        create_nexus().await;

        check_nexus_state_is(NexusState::Online);

        inject_error(SPDK_BDEV_IO_TYPE_READ, VBDEV_IO_FAILURE, 10).await;
        inject_error(SPDK_BDEV_IO_TYPE_WRITE, VBDEV_IO_FAILURE, 10).await;

        for _ in 0 .. 3 {
            err_read_nexus_both(false).await;
            err_write_nexus(false).await;
        }
        for _ in 0 .. 2 {
            // the second iteration causes the error count to exceed the max no
            // of retry errors (4) for the read and causes the child to be
            // removed
            err_read_nexus_both(false).await;
        }
    });

    reactor_run_millis(1); // error child should be removed from the IO path here

    check_nexus_state_is(NexusState::Degraded);

    Reactor::block_on(async {
        err_read_nexus_both(true).await; // should succeed because both IOs go to the remaining child
        err_write_nexus(true).await; // should succeed because the IO goes to
                                     // the remaining child
    });

    mayastor_env_stop(0);

    common::delete_file(&[DISKNAME1.to_string()]);
    common::delete_file(&[DISKNAME2.to_string()]);
    common::delete_file(&[YAML_CONFIG_FILE.to_string()]);
}

fn check_nexus_state_is(expected_state: NexusState) {
    let nexus = nexus_lookup(ERROR_COUNT_TEST_NEXUS).unwrap();
    assert_eq!(nexus.status(), expected_state);
}

async fn create_error_bdev() {
    let mut retval: i32;
    let cname = CString::new(ERROR_DEVICE).unwrap();
    let filename = CString::new(DISKNAME2).unwrap();

    unsafe {
        // this allows us to create a bdev without its name being a uri
        retval = create_aio_bdev(cname.as_ptr(), filename.as_ptr(), 512)
    };
    assert_eq!(retval, 0);

    let err_bdev_name_str = CString::new(ERROR_DEVICE.to_string())
        .expect("Failed to create name string");
    unsafe {
        retval = spdk_vbdev_error_create(err_bdev_name_str.as_ptr()); // create the error bdev around it
    }
    assert_eq!(retval, 0);
}

async fn create_nexus() {
    let ch = vec![BDEVNAME1.to_string(), BDEV_EE_ERROR_DEVICE.to_string()];

    nexus_create(ERROR_COUNT_TEST_NEXUS, 64 * 1024 * 1024, None, &ch)
        .await
        .unwrap();
}

async fn err_read_nexus() -> bool {
    let bdev = Bdev::lookup_by_name(ERROR_COUNT_TEST_NEXUS)
        .expect("failed to lookup nexus");
    let d = bdev
        .open(true)
        .expect("failed open bdev")
        .into_handle()
        .unwrap();
    let mut buf = d.dma_malloc(512).expect("failed to allocate buffer");

    d.read_at(0, &mut buf).await.is_ok()
}

async fn err_read_nexus_both(succeed: bool) {
    let res1 = err_read_nexus().await;
    let res2 = err_read_nexus().await;

    if succeed {
        assert!(res1 && res2); // both succeeded
    } else {
        assert_ne!(res1, res2); // one succeeded, one failed
    }
}

async fn err_write_nexus(succeed: bool) {
    let bdev = Bdev::lookup_by_name(ERROR_COUNT_TEST_NEXUS)
        .expect("failed to lookup nexus");
    let d = bdev
        .open(true)
        .expect("failed open bdev")
        .into_handle()
        .unwrap();
    let buf = d.dma_malloc(512).expect("failed to allocate buffer");

    match d.write_at(0, &buf).await {
        Ok(_) => {
            assert_eq!(succeed, true);
        }
        Err(_) => {
            assert_eq!(succeed, false);
        }
    };
}

async fn inject_error(op: u32, mode: u32, count: u32) {
    let retval: i32;
    let err_bdev_name_str =
        CString::new(EE_ERROR_DEVICE).expect("Failed to create name string");
    let raw = err_bdev_name_str.into_raw();

    unsafe {
        retval = spdk_vbdev_error_inject_error(raw, op, mode, count);
    }
    assert_eq!(retval, 0);
}
fn reactor_run_millis(milliseconds: u64) {
    let (s, r) = unbounded::<()>();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(milliseconds));
        s.send(())
    });
    reactor_poll!(r);
}
