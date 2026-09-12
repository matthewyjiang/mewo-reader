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

Scopes are `android`, `ios`, or `server`. Drop the scope if the change spans more than one.

The description is imperative, lowercase, and has no trailing period.

```
feat(android): hide the title bar when you scroll down
fix(server): reject empty hosted handles
ci(android): attach a signed apk on v* tags
```

A breaking change uses `feat(android)!:` or a `BREAKING CHANGE:` footer.
