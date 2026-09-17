---
schema: 1
id: WD-01ARZ3NDEKTSV4RRFFQ69G5FAV
revision: 1
title: Preserve the login destination
status: ready
priority: medium
created_at: 2026-09-08T10:00:00Z
updated_at: 2026-09-08T10:00:00Z
documents: [docs/authentication.md]
acceptance:
  - id: login-deep-link
    description: Deep links survive login
    checked: false
---
Preserve the requested destination through the login callback.

The check/profile references are declarative contract fixtures. No check runner
or completion qualification is claimed by this fixture.
