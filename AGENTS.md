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

Scopes are `android`, `ios`, `web`, or `server`. Drop the scope when a change spans more than one.

The description is imperative, lowercase, and has no trailing period.

```
feat(android): hide the title bar when you scroll down
fix(server): reject empty hosted handles
ci(android): attach a signed apk on v* tags
```

A breaking change uses `feat(android)!:` or a `BREAKING CHANGE:` footer.

## Branches and pull requests

Do the work on a branch and open a pull request.

## Attribution

Do not add a tool, assistant, or generated-by line to a commit message or a pull request.

## Feature parity

Try to implement new features and behavior changes on all clients, so the clients stay in parity.

## Before a pull request

Run the tests and a build for the components you changed before opening the pull request.

## Android layout screenshots

Prefer Paparazzi or Robolectric for layout screenshots. Use an emulator only when the shot has to come from a system image. For an Android UI change, record Paparazzi goldens, inspect the PNGs, zoom in, and apply the UI quality rule.

```bash
cd android
./gradlew :app:recordPaparazziDebug
```

`./gradlew :app:verifyPaparazziDebug` checks `android/app/src/test/snapshots`. Paparazzi runs on JDK 21. The app still targets JVM 17. Headless emulator steps are in [android/README.md](android/README.md).

## UI quality

Check every UI change on web, Android, and iOS by eye before the pull request opens. Screenshot the simulator, emulator, or browser, including a narrow width. Zoom in. Controls in a row share one vertical center line. Sibling actions match in size. Spacing stays even. Text is not clipped or overflowing. In a list row, prefer a compact icon button over a large switch or a text label. Attach the screenshots to the pull request.
