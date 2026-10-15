# Android emulator on this cloud VM

`scripts/cloud-agent-install.sh`, run from `.cursor/environment.json`, downloads the system image and creates the `mewo` AVD. It does not boot the emulator. Boot with `bash scripts/android-emulator.sh`. The guest uses about 4 GB of RAM.

Nested KVM on this kernel is broken. `/dev/kvm` opens and `emulator -accel-check` says KVM is usable, then `kvm_arch_vcpu_create` hits a kernel BUG and the guest never runs. `scripts/android-emulator.sh` passes `-accel off`. Leave `MEWO_EMULATOR_ACCEL` unset. `MEWO_EMULATOR_ACCEL=on` opts into KVM on a host where vCPU creation works.

The first cold boot, which dexopts the system image, reached `sys.boot_completed` in 13 minutes 38 seconds. A later cold boot of the same AVD, started by the script, took 4 minutes 4 seconds. Both are too slow for a routine UI check. Prefer Paparazzi. See [android/README.md](../android/README.md) for the generic commands.
