# Run me with 'nix-build nix/tests/basic-mayastor.nix'

import <nixpkgs/nixos/tests/make-test-python.nix> ({ pkgs, version ? 4, ... }:

let
  mayastornode =
    { pkgs, ... }:
    { 
      virtualisation = {
        # Probably not a problem for our tests, but many nixos test use 2047
        # (seemingly due to qemu-system-i386's 2047M memory limit).
        # Maybe it's a idiom, or maybe I'm just a monkey on a ladder.
        memorySize = 2047;
        emptyDiskImages = [ 10240 10240 ]; # 2 x 10G data disks
      };
      networking.firewall.enable = false; # One day, we'll only open what we need

      # boot.kernelModules = [ "nvme_tcp" ]; # Do we need this?
    };


  # TOMTODO Rename
  tm_pkgs = import ../../default.nix;
  mayastor-develop = tm_pkgs.mayastor.override { release = false; };

  commonTestHeader =
    ''
      # Import the mayastor utils.
      # TOMTODO We should be using something like pythonPackages.buildPythonPackage.
      import importlib.util
      spec = importlib.util.spec_from_file_location("mylib", "${./mylib.py}")
      mylib = importlib.util.module_from_spec(spec)
      spec.loader.exec_module(mylib)

      mylib.my_function() # TOMTODO Unused for now, just here as an example.

      start_all()

      global machines
      with log.nested("starting mayastor instances"):
        for machine in machines:
            machine.copy_from_host(
                "${ mayastor-develop }/bin/mayastor",
                "/mnt/mayastor",
            )
            machine.execute("/mnt/mayastor &")

      node1.succeed("sleep 10") ## TOMTODO Replace with polling wait_until_succeeds() or wait_for_open_port() or something.
    '';
in

{
  name = "bringup";
  meta = with pkgs.stdenv.lib.maintainers; {
    maintainers = [ tjoshum ];
  };
  skipLint = true; # TOMTODO Remove one day

  nodes =
    { node1 = mayastornode;
      node2 = mayastornode;
      node3 = mayastornode;
    };

  testScript =
    ''
      ${ commonTestHeader }

      print(node1.succeed("${ mayastor-develop }/bin/mayastor-client pool list"))
    '';
})
