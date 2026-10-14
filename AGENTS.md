# Agents

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/).

```
<type>(<scope>): <description>
```

Types:

- `feat`: user-facing behavior
- `fix`: a bug
- `docs`: docs only
- `refactor`: no behavior change
- `test`: tests only
- `ci`: workflows and release plumbing
- `chore`: deps, tooling, leftovers

Scopes are `android`, `ios`, `web`, or `server`. Drop the scope if the change spans more than one.

The description is imperative, lowercase, and has no trailing period.

```
feat(android): hide the title bar when you scroll down
fix(server): reject empty hosted handles
ci(android): attach a signed apk on v* tags
```

A breaking change uses `feat(android)!:` or a `BREAKING CHANGE:` footer.

## Feature Parity

Any new features or changes added should try to be implemented in all clients, in order to keep feature parity.

## Android emulator

Boot the headless `mewo` AVD and wait for `sys.boot_completed`:

```bash
bash scripts/android-emulator.sh
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n com.mewo.reader/.MainActivity
adb exec-out screencap -p > /tmp/mewo.png
```

The AVD is a medium phone (1080x2400, API 36, Google APIs, x86_64), not a Pixel with a camera hole. Cloud install only downloads the image and creates the AVD. Boot is on demand. It uses about 4 GB of RAM.

Nested KVM on this cloud kernel is broken. `/dev/kvm` opens and `emulator -accel-check` says KVM is usable, then `kvm_arch_vcpu_create` hits a kernel BUG and the guest never runs. The script uses `-accel off`. The first cold boot, which dexopts the system image, reached `sys.boot_completed` in 13 minutes 38 seconds. A later cold boot of the same AVD, started by the script, took 4 minutes 4 seconds. Both are too slow for a routine UI check. Prefer Paparazzi or Robolectric for layout screenshots. Use the emulator when the shot has to come from a system image. Leave `MEWO_EMULATOR_ACCEL` unset.
