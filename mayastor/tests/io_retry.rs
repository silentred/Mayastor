pub use common::error_bdev::{
    create_error_bdev,
    inject_error,
    SPDK_BDEV_IO_TYPE_READ,
    SPDK_BDEV_IO_TYPE_WRITE,
    VBDEV_IO_FAILURE,
};

use mayastor::{
    bdev::nexus_create,
    core::{
        mayastor_env_stop,
        BdevHandle,
        CoreError,
        MayastorCliArgs,
        MayastorEnvironment,
        Reactor,
    },
};

static NEXUS_NAME: &str = "ioq_nexus";
static DISKNAME1: &str = "/tmp/disk.img";
static BDEVNAME1: &str = "aio:///tmp/disk.img?blk_size=512";

static DISKNAME2: &str = "/tmp/disk2.img";

static DISKSIZE_KB: u64 = 64 * 1024;

static ERROR_DEVICE: &str = "error_device";
static EE_ERROR_DEVICE: &str = "EE_error_device"; // The prefix is added by the vbdev_error module
static BDEV_EE_ERROR_DEVICE: &str = "bdev:///EE_error_device";

pub mod common;

/// Test retried internal I/O
#[test]
fn io_retry_test() {
    common::truncate_file(DISKNAME1, DISKSIZE_KB);
    common::truncate_file(DISKNAME2, DISKSIZE_KB);

    test_init!();

    Reactor::block_on(async {
        create_error_bdev(ERROR_DEVICE, DISKNAME2);
        create_nexus().await;
        write_some(None).await.unwrap();
        read_some(None).await.unwrap();

        // Test IO fails after injecting 1 error
        inject_error(
            EE_ERROR_DEVICE,
            SPDK_BDEV_IO_TYPE_WRITE,
            VBDEV_IO_FAILURE,
            1,
        );
        write_some(None)
            .await
            .expect_err("should fail write after injecting 1 error");
        inject_error(
            EE_ERROR_DEVICE,
            SPDK_BDEV_IO_TYPE_READ,
            VBDEV_IO_FAILURE,
            1,
        );
        read_some(None)
            .await
            .expect_err("should fail read after injecting 1 error");

        for i in 2 ..= 3 {
            // Test IO succeeds after retrying 1 more time than number of
            // injected errors
            inject_error(
                EE_ERROR_DEVICE,
                SPDK_BDEV_IO_TYPE_WRITE,
                VBDEV_IO_FAILURE,
                i - 1,
            );
            write_some(Some(i))
                .await
                .expect("should succeed write after injecting errors");
            inject_error(
                EE_ERROR_DEVICE,
                SPDK_BDEV_IO_TYPE_READ,
                VBDEV_IO_FAILURE,
                i - 1,
            );
            read_some(Some(i))
                .await
                .expect("should succeed read after injecting errors");

            // Test IO fails after retrying as many times as injected errors
            inject_error(
                EE_ERROR_DEVICE,
                SPDK_BDEV_IO_TYPE_WRITE,
                VBDEV_IO_FAILURE,
                i,
            );
            write_some(Some(i))
                .await
                .expect_err("should fail write after injecting errors");
            inject_error(
                EE_ERROR_DEVICE,
                SPDK_BDEV_IO_TYPE_READ,
                VBDEV_IO_FAILURE,
                i,
            );
            read_some(Some(i))
                .await
                .expect_err("should fail read after injecting errors");
        }
    });

    mayastor_env_stop(0);

    common::delete_file(&[DISKNAME1.to_string()]);
}

async fn create_nexus() {
    let ch = vec![BDEVNAME1.to_string(), BDEV_EE_ERROR_DEVICE.to_string()];

    nexus_create(NEXUS_NAME, DISKSIZE_KB * 1024, None, &ch)
        .await
        .unwrap();
}

async fn write_some(retries: Option<u32>) -> Result<(), CoreError> {
    let mut bdev = BdevHandle::open(NEXUS_NAME, true, false)
        .expect("failed to open bdev for write");
    bdev.set_retries(retries.unwrap_or(1));
    let mut buf = bdev.dma_malloc(512).expect("failed to allocate buffer");
    buf.fill(0xff);

    let s = buf.as_slice();
    assert_eq!(s[0], 0xff);

    bdev.write_at(0, &buf).await?;
    Ok(())
}

async fn read_some(retries: Option<u32>) -> Result<(), CoreError> {
    let mut bdev = BdevHandle::open(NEXUS_NAME, true, false)
        .expect("failed to open bdev for write");
    bdev.set_retries(retries.unwrap_or(1));
    let mut buf = bdev.dma_malloc(1024).expect("failed to allocate buffer");
    let slice = buf.as_mut_slice();

    assert_eq!(slice[0], 0);
    slice[512] = 0xff;
    assert_eq!(slice[512], 0xff);

    let len = bdev.read_at(0, &mut buf).await?;
    assert_eq!(len, 1024);

    let slice = buf.as_slice();

    for &it in slice.iter().take(512) {
        assert_eq!(it, 0xff);
    }
    assert_eq!(slice[512], 0);
    Ok(())
}
