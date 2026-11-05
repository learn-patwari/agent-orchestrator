# SRS: Profiles, role switching, one login for several clinics, and view-as (web and mobile)

Status: Approved (user sign-off 2026-11-05 pending merge)   Owner (human): product owner (name to confirm)   Date: 2026-11-05   Version: 3

> Written by the architect in BA/SRS mode. Docs only: nothing here is implemented. **D-n** is a decision the user gave. **[A]** is a default I chose where the user has not answered; the user may overrule it. Section 3 records what `stage` (commit `8602aa4`) already does. The approval above was relayed by the Orchestrator; merging PR 23 is the user's own record (section 13).

## 1. Purpose and background
The user asked for "profiles for platform admin, testers and clinic admin and doctor; choose a profile in the top header". Their answers make it a mix of four things:

1. **Account menu** in the top header (view and edit my profile, sign out). Exists today.
2. **Switching roles** for people who hold more than one.
3. **One login for a doctor who works at several clinics**, then pick the clinic.
4. **View as**: the platform admin looks at any dashboard as that role sees it, read-only.

Development starts only after current stage testing completes (D-9).

## 2. Decisions and defaults
| ID | Decision |
|---|---|
| D-1 | The request is a mix of account menu, role switching and view-as. |
| D-2 | Multi-role people to support: clinic admin + doctor; platform admin + tester; doctor at several clinics; staff + doctor. |
| D-3 | Platform admin can view as any role. View-as stays read-only. |
| D-4 | Profile fields stay as today: name, email, phone, password; doctor keeps qualification, specialization, experience, bio. |
| D-5 | No photo. |
| D-6 | Edit rights: everyone edits their own basic profile; a doctor edits their own fee and details; a clinic admin edits their clinic's doctors; a platform admin edits testers. |
| D-7 | Web and mobile are both in scope. |
| D-8 | Audit all three: profile switches, view-as sessions, and edits to someone else's profile. |
| D-9 | Build later, after current stage testing completes. |
| D-10 | **View-as: platform admin only.** Any role, any clinic, read-only, patient data visible to the platform admin. No one else (not clinic admin, staff or doctor). |
| D-11 | **Doctor at several clinics: one login** (one email and password), then pick a clinic. Clinics are linked **only after the doctor confirms**; never automatically by email or phone. |
| D-12 | **Staff + doctor: keep both hats.** The existing role overwrite is a bug to fix. |
| D-13 | **Only the clinic admin may grant the doctor hat.** |

Defaults the user may overrule:

| ID | Default **[A]** |
|---|---|
| A-1 | View-as is **not** on mobile. |
| A-2 | Hats on platform roles (platform admin + tester) are granted by the platform admin only. A tester can also remain a separate account. |
| A-3 | The audit log is readable by the platform admin only and kept for 1 year. |
| A-4 | A doctor edits their own fee and details (per D-6). Their schedule stays a clinic admin task unless the user says otherwise. |

## 3. What exists today (evidence from `stage`)
- **Header menu** (`frontend/app/dashboard/layout.tsx`): name, email, role badge, **My profile** (route per role), **Switch to doctor / clinic admin view** when the account has two hats, **Sign out**; an amber banner during view-as.
- **`GET/PATCH /api/v1/users/me`**: returns `profiles`, `permissions`, `role_names`, `login_id`; PATCH changes name, email, phone, password only.
- **Hats**: `_profiles(user)` is the base role plus `doctor` when `role == clinic_admin` and `also_doctor`. `POST /api/v1/auth/switch-profile` issues new tokens. Not audited.
- **View as**: `POST /api/v1/admin/view-as`, guarded by `require_super_admin`, any role in `VIEWABLE_ROLES`. The token carries `impersonator` and lasts `VIEW_AS_MINUTES`; `_gate` makes every request read-only and writes `audit_log` rows (`view_as`, `view_as_request`). Page: `/dashboard/super-admin/view-as`, not in the header. This already matches D-10.
- **Profile pages**: tester re-uses the super admin page; clinic admin and staff re-use the patient page; doctor has its own page (`/doctors/me`).
- **Doctor at several clinics today**: a separate `users` row and `doctors` row per clinic, linked only by matching email or phone (`_sibling_doctors`, used to stop double-booking). Each clinic is a separate sign-in (clinic ID + credentials) with its own password. Unique index `users(tenant_slug, email)`. Patients have an OTP clinic picker (`/auth/patient/clinics`, token `purpose: patient_clinic_select`); doctors do not.
- **Staff + doctor today (bug)**: when a clinic adds an existing `staff` user as a doctor, `create_doctor` sets `users.role = "doctor"`, replacing the staff role and its permissions. Only `clinic_admin` keeps its role through `also_doctor`.
- **Platform admin + tester today**: both live in tenant `__platform__`, one row per person, so one account cannot hold both.
- **JWT**: access claims `sub`, `role`, `tenant_slug`, `exp`, plus `impersonator` in view-as. `get_current_user` rejects any token carrying `purpose`. Refresh tokens are stored per `(user_id, role, tenant_slug)`.
- **Mobile**: `select-clinic` (patient) and role groups for patient, doctor, clinic-admin and super-admin. No profile screen, no switch, no tester group, no one-login flow for doctors.
- **Audit**: `audit_log`, read by the platform admin through `GET /api/v1/admin/audit`. Switches and edits of others are not logged.

## 4. Users and roles affected
| Role | Profile today | Change |
|---|---|---|
| `super_admin` | Own page | Tester hat in header (A-2); view-as stays theirs alone; edits testers |
| `tester` | Re-uses super admin page | Own page with the same fields; may be a hat of a platform admin |
| `clinic_admin` | Re-uses patient page | Own page; doctor hat (exists); the only role that grants the doctor hat; edits own clinic's doctors |
| `doctor` | Own page | One login, clinic picker and in-app clinic switch; admin or staff hat where held; edits own fee and details |
| `staff` | Re-uses patient page | Keeps staff hat when also a doctor |
| `patient` | Own page | Target of view-as only; no change |

## 5. Scope
**In scope:** header account menu on web and mobile; hat switching for the four combinations in D-2; one login and a confirmed clinic link for doctors; view-as entry in the header for the platform admin; the audit events in D-8; separate profile pages for tester and clinic admin; mobile profile screen, switch, tester group and the one-login flow.

**Out of scope:** photo (D-5); new profile fields (D-4); view-as for anyone but the platform admin (D-10) and view-as on mobile (A-1); automatic account linking; changes to `permissions.py` beyond what a hat needs; making view-as writable.

## 6. Functional requirements
| ID | Requirement (shall ...) | Role | Priority | Acceptance criterion |
|---|---|---|---|---|
| FR-1 | Show name, role badge and clinic in the header menu with a working "My profile" link, on web and mobile. | all | Must | Given each role, when the menu opens, then name, active role and link are shown. |
| FR-2 | Give tester and clinic admin their own profile screens with the D-4 fields. | tester, clinic_admin | Should | Given a tester, when My profile opens, then name, email, phone, password save and nothing else appears. |
| FR-3 | List every hat a person holds in the current clinic or platform and switch without signing out. | clinic_admin+doctor, super_admin+tester, staff+doctor | Must | Given two hats, when switched, then dashboard, nav and badge change and the token carries the new role. |
| FR-4 | Let a doctor sign in once (email and password, no clinic ID), see the clinics they have confirmed, and pick one. | doctor | Must | Given a linked doctor, when they sign in with email and password, then a clinic list appears and choosing one opens that clinic's dashboard. |
| FR-5 | Let a signed-in doctor link another clinic account only after confirming control of it. | doctor | Must | Given a doctor at clinic A who enters clinic B's ID, when they enter the code emailed to the account at B, then the accounts are linked; with a wrong code, nothing is linked. |
| FR-6 | Never link accounts automatically from matching email or phone. | doctor | Must | Given two accounts with the same email at different clinics and no confirmation, then they are not linked and each signs in separately. |
| FR-7 | Let a linked doctor switch clinic from the header without signing in again. | doctor | Must | Given clinics A and B linked, when they pick B, then every request uses B and none of A's data is reachable. |
| FR-8 | A hat or clinic is switchable only if the server lists it for that person; otherwise 403. | all | Must | Given an unlisted hat or clinic, then 403 and an audit row (outcome denied). |
| FR-9 | Adding an existing staff user as a doctor shall keep the staff role and add the doctor hat. | staff | Must | Given a staff member added as a doctor, when they sign in, then both hats are available and staff access is unchanged. |
| FR-10 | Only the clinic admin of that clinic may grant or remove the doctor hat. | clinic_admin | Must | Given a doctor, staff or platform admin calling the grant, then 403; given the clinic admin, then it succeeds. |
| FR-11 | View-as is available only to the platform admin (any role, any clinic), started from the header, read-only, time-limited and bannered. | super_admin | Must | Given a platform admin, when they start view-as, then the banner shows, writes get 403 and the session expires; given any other role, then 403. |
| FR-12 | Everyone edits their own name, email, phone, password. | all | Must | Existing `PATCH /users/me` behavior holds. |
| FR-13 | A doctor edits their own fee and details; a clinic admin edits their clinic's doctors; a platform admin edits testers; nobody else edits another person. | doctor, clinic_admin, super_admin | Must | Each pairing succeeds, any other returns 403. |
| FR-14 | Audit profile switches (including denied and clinic switches), clinic links, view-as sessions and every request in them, and edits to someone else's profile. | all | Must | Each event writes an `audit_log` row with actor, target, action, clinic, outcome and time, and never a password, code or token. |
| FR-15 | Mobile: profile screen, hat and clinic switch, a tester group with the bug tracker entry, and the one-login flow with clinic picker. | mobile | Should | Given the mobile app, then the same hats and clinics are reachable as on web. |

## 7. Non-functional requirements
- **Tenant isolation**: a token is for one `(user row, hat, clinic)`. Switching never carries data or permissions across; `_gate` still refuses a token whose clinic differs from the header's clinic.
- **Privilege**: a switch must not widen access. Platform hats (`__platform__`) and clinic hats never mix in one token.
- **Login safety**: the email-only login must not reveal whether an email exists or which clinics it belongs to; it uses the existing failed-login rate limit.
- **View-as leakage**: platform admin only; sessions short, writes refused, every request logged; if a row cannot be written the request fails (as today).
- **Audit**: no secrets stored; platform admin readable; kept 1 year (A-3).
- **Compatibility**: old tokens and per-clinic sign-in keep working throughout the migration (expand/contract).
- **Accessibility / responsive**: header menu usable by keyboard and at phone width; mobile equivalent.
- **Performance**: no extra request per page beyond `users/me`.

## 8. Data model, auth and interfaces
### 8.1 Options considered
| Option | Idea | Pros | Cons |
|---|---|---|---|
| A. `users.hats` only | One row holds hats for all clinics | Simple for same-clinic combos | Breaks `users(tenant_slug, email)` and tenant isolation; merges accounts with separate passwords |
| B. `person_id` link only | Keep one user row per clinic; link rows | Keeps isolation and indexes | Cannot model two roles in the same clinic |
| **C. Hybrid (chosen)** | `users.hats` within one clinic or the platform, plus a `persons` identity that owns the single login and links the per-clinic rows | Covers all D-2 combinations and D-11; keeps isolation | Two concepts to explain and test |

### 8.2 Schema
- `users.hats`: roles held in this row's tenant, e.g. `["clinic_admin","doctor"]`. Base `role` stays the default hat. Replaces `also_doctor`, kept in step during migration.
- `users.person_id`: set only on rows the doctor has confirmed linking. Null for everyone else.
- New collection `persons`: `{id, email, password_hash, created_at}`. It holds the one login (D-11). Membership lives on the user rows (`person_id`), so there is one source of truth for which clinics belong to a person.
- New short-lived collection or reuse of `email_otps` for link confirmation codes (expiry, attempt limit).
- `audit_log`: new actions `profile_switch`, `clinic_switch`, `clinic_link`, `profile_edit_by_other`; fields `actor_id`, `target_id`, `from`, `to`, `fields_changed` (names only), `outcome`.
- Unchanged: `doctors`, `_sibling_doctors` (still used to stop double-booking, independent of linking).
- Docs to update: `docs/schema/erd.md`, `docs/api`, `docs/STAFF_ACCESS.md`.

### 8.3 Auth, JWT and API
- **Existing sign-in unchanged**: clinic ID + credentials still works for everyone, including linked doctors, until the contract phase.
- **One login (new)**:
  1. `POST /auth/login-person` with email and password (no clinic ID) checks `persons`. On success it returns a short-lived token `purpose: "person_clinic_select"` (10 minutes, carries `person_id` only) and the list of confirmed clinics: slug, clinic name, role. Wrong credentials return a generic 401.
  2. `POST /auth/select-clinic` with that token and a clinic slug loads the user row with that `person_id` and tenant, rechecks `is_active`, and issues normal tokens (`sub` = that row, `role` = its default hat, `tenant_slug`). `get_current_user` already rejects `purpose` tokens, so the picker token cannot call anything else.
- **Linking (new, D-11)**: signed in at clinic A, the doctor starts `POST /auth/link-clinic` with clinic B's ID. The server emails a code to the account's email at clinic B; `POST /auth/link-clinic/confirm` with the code sets the same `person_id` on both rows and creates the `persons` record from the signed-in account if none exists. The person password starts as the signed-in account's password. Nothing is linked by matching email or phone. Rate limits and a short code expiry apply.
- **Password**: `change-password` on a linked account updates `persons` and every linked row's `password_hash`, so per-clinic sign-in and the one login cannot drift apart.
- **In-app switching**: `GET /users/me` returns `hats` (this clinic) and `memberships` (other confirmed clinics). `POST /auth/switch-profile` accepts any hat in `hats`. `POST /auth/switch-clinic` accepts a clinic from `memberships`, loads that row (same `person_id`), rechecks `is_active`, and issues tokens bound to that row. A refresh token stays bound to the row and clinic it was issued for.
- **JWT**: claim shape is unchanged for normal tokens (`sub`, `role`, `tenant_slug`); `impersonator` only for view-as. The only new token type is the `purpose` picker token.
- **View-as**: unchanged guard (`require_super_admin`); impersonated tokens cannot refresh or write (tests will prove it).
- **Mobile**: login gets an email-and-password path and a clinic picker (reusing the `select-clinic` screen pattern), plus a profile screen, switcher and `(tester)` group. No view-as and no parked tokens (A-1).

### 8.4 Migration plan (expand / contract)
1. **Expand**: add `hats`, `person_id` and `persons` as optional; code reads `hats` if present, else derives it from `role` and `also_doctor`. Deploy backend only.
2. **Backfill** (`scripts/backfill-hats.py`, `--dry-run` first, stage before production, safe to repeat): sets `hats` from `role` + `also_doctor`. It does **not** create links or `persons`; those appear only when a doctor confirms (D-11). It reports doctors that look like the same person (via `_sibling_doctors`) so the clinic can invite them to link.
3. **Switch reads**: UI and APIs use `hats` and `memberships`; the one-login endpoints go live.
4. **Contract**: stop writing `also_doctor`; remove it one release later. Rollback keeps `also_doctor` in step until then.

### 8.5 Interfaces
| Area | Change |
|---|---|
| Backend | `_profiles` reads `hats`; `switch-profile` generalised; `login-person`, `select-clinic`, `link-clinic`, `switch-clinic`; password sync; FR-9 fix in `create_doctor`; doctor-hat grant rule (FR-10); edit-rights checks (FR-13); audit writes |
| Web | Login without clinic ID, clinic picker, link-clinic screen; header with hats, clinics and View as; tester and clinic admin profile pages |
| Mobile | Email-and-password login, clinic picker, `profile` screen, switcher, `(tester)` group |
| Docs | ERD, API, `STAFF_ACCESS.md`, platform admin guide, doctor guide |

## 9. Assumptions and constraints
1. Defaults A-1 to A-4 in section 2 stand until the user says otherwise.
2. Clinic ID + credentials sign-in stays available; the one login is added beside it.
3. Dependency: stage testing completes first (D-9).
4. Constraint: PRs to `stage`; refactors and features in separate PRs; no direct pushes.

## 10. Open questions
| # | Question | Asked of | Answer | Status |
|---|---|---|---|---|
| Q1 to Q8 | See D-1 to D-9 | Product owner | Answered | Closed |
| OQ-1 | View-as who and which roles | Product owner | D-10: platform admin only, any role and clinic, read-only, patient data visible | Closed |
| OQ-2 | Doctor at several clinics | Product owner | D-11: one login then pick a clinic; link only after the doctor confirms | Closed |
| OQ-3 | Staff + doctor | Product owner | D-12 and D-13: keep both hats; only the clinic admin grants the doctor hat | Closed |
| OQ-4 | View-as on mobile? | Product owner | A-1: no | Default [A], user may overrule |
| OQ-5 | Who grants platform hats? | Product owner | A-2: platform admin only | Default [A], user may overrule |
| OQ-6 | Who reads the audit log, retention? | Product owner | A-3: platform admin only, 1 year | Default [A], user may overrule |
| OQ-7 | Doctor edits own fee; own schedule? | Product owner | A-4: own fee and details yes; schedule stays with clinic admin | Default [A], user may overrule |

## 11. Risks
| Risk | Impact | Mitigation |
|---|---|---|
| One login or picker token used to reach the wrong clinic | Cross-clinic data leak | Picker token carries only `person_id`, is rejected by `get_current_user`, and `select-clinic` only returns rows with that `person_id` |
| Linking two different people | One person sees another's clinics | Link only after emailed code to the clinic B account (FR-5); no auto-link; code expiry and attempt limit; audit row |
| Email-only login leaks which emails exist | Account enumeration | Generic 401, rate limit on failures, same response time path |
| Password drift between `persons` and per-clinic rows | Lockouts or two valid passwords | `change-password` updates all linked rows and `persons`; test for it; contract phase removes the legacy path |
| Token scope drift after switching hats or clinics | Privilege escalation | One hat per token; `_gate` rules for tester and staff re-apply; platform and clinic hats never combined |
| Impersonated token refresh or write | Escape from read-only | Tests: refresh refused, every write 403 |
| View-as exposes patient data | Privacy breach | Platform admin only (D-10), short expiry, per-request audit that fails closed, reason recorded |
| Migration drops the doctor hat | Doctors lose access | Expand/contract, `also_doctor` kept in sync until contract, backfill dry-run and rollback |
| FR-9 fix changes existing staff accounts | Unexpected role change | Fix only affects new grants; backfill dry-run lists any account already overwritten |
| Audit rows leak secrets | Credential exposure | Store field names only; test that no password, code or token appears |
| Mobile keeps tokens after sign-out | Data left on device | Clear on sign-out; test |

## 12. Development plan
Each phase is one small PR to `stage`, built by the developer agent, verified by the tester, reviewed by the reviewer, in the order of `.development/WORKFLOW.md`. Development starts after stage testing completes. All backend phases edit `backend/server.py`, so they run one at a time, and each phase puts its new tests in its own file (for example `tests/test_hats.py`) to avoid conflicts in `tests/test_server.py`.

| Phase | Tasks (owner) | Depends on | Parallel-safe | Test plan | Done when |
|---|---|---|---|---|---|
| **P0** Sign-off | User merges PR 23 (user) | none | n/a | n/a | PR 23 merged |
| **P1** Audit foundation | Add audit actions and a helper; log existing `switch-profile` and own-profile edits (developer); pytest (tester) | P0 | Backend lane: sequential with P2, P4, P5 | pytest: row per event, denied switch logged, no secrets in rows | Every switch and edit writes a row; platform admin reads them |
| **P2** Data model, expand | `hats`, `person_id`, `persons` fields; `_profiles` reads `hats` with `also_doctor` fallback; `users/me` returns `hats` and `memberships`; `backfill-hats.py` with dry-run (developer); migration tests (tester) | P1 | Backend lane | pytest: hats derived from old data, backfill idempotent and creates no links, old tokens work; dry-run on a stage copy | Old and new data resolve to the same hats; no behavior change |
| **P3** Own profile pages (web) | Tester and clinic admin profile pages with D-4 fields (developer, `frontend/app/dashboard/*/profile` only); Playwright (tester) | P0 only (uses existing `users/me`) | **Yes**: parallel with P1, P2, P4, P5 (frontend files only) | Playwright per role: edit own profile, no foreign fields | Each of the four roles has its own page |
| **P4** Hats backend and fixes | `switch-profile` for any hat; FR-9 fix in `create_doctor`; doctor-hat grant rule FR-10; edit-rights FR-13; platform hats granted by platform admin (A-2) (developer); pytest (tester) | P2 | Backend lane | pytest: each hat combination switches; unlisted hat 403; staff keeps permissions; grant allowed only for clinic admin; edit pairings | All three same-clinic combinations work safely |
| **P5** One login and clinic linking (backend) | `login-person`, `select-clinic`, `link-clinic` and confirm, `switch-clinic`, password sync, rate limits (developer); pytest and a security review (tester, reviewer) | P4 | Backend lane | pytest: picker token cannot call APIs; only confirmed clinics listed; wrong code links nothing; no auto-link; generic 401; password sync; cross-clinic data unreachable after switch | A linked doctor signs in once and moves between clinics safely |
| **P6** Web login, picker and header | Email-and-password login without clinic ID, clinic picker, link-clinic screen; header with hats, clinics (developer, `login`, new pages, `layout.tsx`); Playwright (tester) | P5 | Parallel with P8 (different directories); sequential before P7 (both touch `layout.tsx`) | Playwright: one login, pick clinic, link flow, switch hat, switch clinic | Web matches the SRS for hats and clinics |
| **P7** View-as in header and hardening | Header entry for the platform admin only; confirm every other role is refused; read-only, expiry and refresh tests (developer, tester, reviewer) | P6 | Sequential after P6 (`layout.tsx`); backend part is tests only | pytest: non-platform 403, writes 403, refresh refused, per-request audit; Playwright: banner, expiry, exit | View-as limited to the platform admin and reachable from the header |
| **P8** Mobile | Email-and-password login, clinic picker, `profile` screen, switcher, `(tester)` group (developer, `mobile/` only); simulator or device test (tester) | P5 | Parallel with P6 and P7 (mobile files only) | Manual checklist, mobile e2e where available: login, pick, switch, sign-out clears tokens | Mobile matches web for profile, hats and clinics; no view-as |
| **P9** Contract and docs | Stop writing `also_doctor`; update ERD, API, `STAFF_ACCESS.md`, platform admin and doctor guides (developer, architect) | P6, P7, P8 | Last | Full pytest and e2e run; docs reviewed | `also_doctor` unused for one release, docs current |

**Parallel summary for assigning Developer sessions:** lane 1 (backend, one session at a time): P1, P2, P4, P5. Lane 2 (web): P3 can start at once; P6 then P7 after P5. Lane 3 (mobile): P8 after P5. P9 last.

## 13. Sign-off
Human approval recorded here before development starts.

| Date | Approved by | Notes |
|---|---|---|
| 2026-11-05 | Product owner (relayed by the Orchestrator session; confirmed by merging PR 23) | Decisions D-10 to D-13 and defaults A-1 to A-4 as above. Pending merge. |
