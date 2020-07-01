use mayastor::{
    core::{
        mayastor_env_stop,
        Bdev,
        MayastorCliArgs,
        MayastorEnvironment,
        Reactor,
    },
    nexus_uri::bdev_create,
    subsys::{NvmfSubsystem, SubType},
};
use std::convert::TryFrom;

pub mod common;

static DISKNAME1: &str = "/tmp/disk1.img";
static BDEVNAME1: &str = "aio:///tmp/disk1.img?blk_size=512";

#[test]
fn nvmf_target() {
    common::mayastor_test_init();
    common::truncate_file(DISKNAME1, 64 * 1024);
    let mut args = MayastorCliArgs::default();
    args.reactor_mask = "0x3".into();
    MayastorEnvironment::new(args)
        .start(|| {
            // test we can create a nvmf subsystem
            Reactor::block_on(async {
                let b = bdev_create(BDEVNAME1).await.unwrap();
                let bdev = Bdev::lookup_by_name(&b).unwrap();

                let ss = NvmfSubsystem::try_from(&bdev).unwrap();
                ss.start().await.unwrap();
            });

            // test we can not create the same one again
            Reactor::block_on(async {
                let bdev = Bdev::lookup_by_name(BDEVNAME1).unwrap();

                let should_err = NvmfSubsystem::try_from(&bdev);
                assert_eq!(should_err.is_err(), true);
            });

            // verify the bdev is claimed by our target
            Reactor::block_on(async {
                let bdev = Bdev::bdev_first().unwrap();
                assert_eq!(bdev.is_claimed(), true);
                assert_eq!(bdev.claimed_by().unwrap(), "NVMe-oF Target");

                let mut ss = NvmfSubsystem::first().unwrap();
                // skip the discovery controller
                if ss.subtype() == SubType::Discovery {
                    ss = ss.into_iter().next().unwrap();
                }

                ss.stop().await.unwrap();
                let sbdev = ss.bdev().unwrap();

                assert_eq!(sbdev.name(), bdev.name());

                assert_eq!(bdev.is_claimed(), true);
                assert_eq!(bdev.claimed_by().unwrap(), "NVMe-oF Target");

                ss.destroy();
                assert_eq!(bdev.is_claimed(), false);
                assert_eq!(bdev.claimed_by(), None);
            });
            // this should clean/up kill the discovery controller
            mayastor_env_stop(0);
        })
        .unwrap();
}
