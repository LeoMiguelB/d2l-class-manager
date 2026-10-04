# `d2l` (`d2l-class-manager`)

> High-performance Rust CLI data getter, content downloader, and AI verification / reconciliation layer for **D2L Brightspace / CourseLink**.

[![Language](https://img.shields.io/badge/language-Rust%202021-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Status](https://img.shields.io/badge/status-active-brightgreen.svg)]()

---

## Table of Contents

- [1. What the Tooling Is For & How It Works](#1-what-the-tooling-is-for--how-it-works)
  - [The Problem](#the-problem)
  - [The Solution & Purpose](#the-solution--purpose)
  - [How It Works Under the Hood](#how-it-works-under-the-hood)
  - [System Architecture](#system-architecture)
- [2. Installation & Setup (Fresh Machine)](#2-installation--setup-fresh-machine)
  - [Prerequisites](#prerequisites)
  - [Building from Source](#building-from-source)
  - [System Configuration & Paths](#system-configuration--paths)
  - [Environment Variables](#environment-variables)
- [3. Usage Guide](#3-usage-guide)
  - [Authentication (`login` & `status`)](#authentication-login--status)
  - [Inspecting Courses & Metadata (`courses`)](#inspecting-courses--metadata-courses)
  - [Tracking Assignments & Submissions (`assignments`)](#tracking-assignments--submissions-assignments)
  - [Course Announcements & News (`announcements`)](#course-announcements--news-announcements)
  - [Grades & Evaluation Breakdown (`grades`)](#grades--evaluation-breakdown-grades)
  - [Quizzes & Availability Windows (`quizzes`)](#quizzes--availability-windows-quizzes)
  - [Cross-Course Calendar (`calendar`)](#cross-course-calendar-calendar)
  - [Discussion Forums & Readings (`posts`)](#discussion-forums--readings-posts)
  - [Downloading Lecture Slides & Files (`download`)](#downloading-lecture-slides--files-download)
  - [Unified JSON State Export for AI (`dump`)](#unified-json-state-export-for-ai-dump)
  - [Vault Reconciliation Engine (`reconcile`)](#vault-reconciliation-engine-reconcile)
- [4. Integration with `School-Dashboard` & AI Layer](#4-integration-with-school-dashboard--ai-layer)
- [5. Troubleshooting & FAQ](#5-troubleshooting--faq)

---

## 1. What the Tooling Is For & How It Works

### The Problem
University learning portals built on **D2L Brightspace** (such as the University of Guelph's **CourseLink** at `courselink.uoguelph.ca`) suffer from severe data fragmentation:
- **Scattered Information:** Checking deadlines, lecture slides, grades, and discussion topics requires navigating dozens of disparate tabs, submenus, and separate course pages.
- **Dynamic & Informal Changes:** Instructors frequently announce deadline postponements, room relocations, or deliverable modifications in plain-text Announcements rather than updating the formal assignment dropboxes or course syllabus PDFs.
- **Submission Anxiety:** Verifying whether an assignment actually uploaded successfully requires repeated manual browser logins.
- **Lack of Local Automation:** D2L Brightspace does not offer public self-service API client registration for students; standard OAuth2 requires institution-level administrative clearance.

### The Solution & Purpose
`d2l-class-manager` (binary name: `d2l`) is a standalone, blazing-fast CLI written in **Rust** designed to:
1. **Bypass the OAuth2 Barrier Safely:** Intercept temporary user session Bearer JWTs without requiring server-side administrator keys or institutional registration.
2. **Harvest Valence REST Data in Read-Only Mode:** Query D2L's official Valence REST APIs (`lp`, `le`, `bas`) to extract course rosters, assignment dropboxes, submission history, announcement bodies, grades, quizzes, and calendars.
3. **Bridge Local Academic Workspaces:** Connect directly to local course vaults (e.g., Obsidian vaults managed by [`School-Dashboard`](https://github.com/LeeOs/School-Dashboard)) containing `deadlines.json` files.
4. **Act as an AI Verification Layer:** Expose structured, deterministic JSON payloads that AI models and local agents can consume to detect deadline drift, reconcile informal announcement changes, verify file receipts, and suggest automated schedule updates.

---

### How It Works Under the Hood

```
+-----------------------------------------------------------------------------------------+
|                                   d2l Architecture Flow                                 |
+-----------------------------------------------------------------------------------------+

 [User / TUI / AI Agent]
         │
         ▼
    `d2l` CLI Command (e.g. `d2l assignments -c CIS4300 --json`)
         │
         ├──► 1. Token Resolution
         │       ├─ Check $D2L_TOKEN env var
         │       ├─ Check ~/.config/d2l-manager/token.json (validity & exp)
         │       └─ If expired: Launch Chrome via CDP (headless restore or headed 2FA)
         │
         ├──► 2. Valence REST API Engine (reqwest)
         │       ├─ Spoofs browser headers (User-Agent, Origin, Referer) to satisfy WAF
         │       ├─ Resilient exponential backoff on HTTP 429 (Retry-After inspection)
         │       ├─ Handles Bookmark pagination (enrollments) & Page-Number pagination (posts)
         │       └─ Strictly HTTP GET (Read-Only)
         │
         ├──► 3. Course & Vault Resolver
         │       ├─ Matches queries ("CIS*4300", "cis4300", "4300", OrgUnit ID)
         │       ├─ Reads Obsidian vault ($SCHOOL_DASHBOARD_VAULT/<COURSE>/deadlines.json)
         │       └─ Extracts CourseLink links or performs fuzzy course-code resolution
         │
         └──► 4. Output & Reconciliation
                 ├─ Human Mode: Styled UTF-8 tables (`comfy-table`) with colored status
                 ├─ Machine Mode (`--json`): Pure JSON for AI prompts or dashboards
                 └─ Reconcile Mode (`--apply`): Diffs dates/submissions, writes safe .json.bak
```

#### 1. Authentication via Chrome DevTools Protocol (CDP)
Instead of fragile web scraping or requiring institutional developer credentials, `d2l` uses native Chrome DevTools Protocol automation via [`chromiumoxide`](https://crates.io/crates/chromiumoxide):
* **Persistent Browser Profile:** Stored at `~/.config/d2l-manager/browser_profile`.
* **Zero-Touch Headless Restore:** Once you log in once with your university credentials and 2FA (e.g. Duo Mobile), session cookies remain in the browser profile. Subsequent token refreshes run headlessly in ~1.5 seconds in the background without opening a browser window.
* **Network Header Sniffing:** When the D2L Single Page Application loads, it issues background API calls carrying `Authorization: Bearer eyJ...`. The CDP listener intercepts the token from the network event stream, parses the claims without heavy crypto dependencies, and stores it in `~/.config/d2l-manager/token.json`.

#### 2. Read-Only Valence REST Client
Network interactions are strictly read-only and pass through `reqwest` configured to mirror genuine browser requests:
* **Valence API Endpoints:** Queries `/d2l/api/lp/1.47/` (Learning Platform: identity, enrollments) and `/d2l/api/le/1.80/` (Learning Environment: content, dropboxes, news, grades, calendar).
* **WAF/CORS Resilience:** Injects matching browser `User-Agent`, `Origin`, and `Referer` headers to prevent Cloudflare / WAF blockades.
* **Rate-Limit Handling:** Automatically catches HTTP `429 Too Many Requests`, parses the `Retry-After` header, and retries safely.

#### 3. Course & Vault Matching Engine
The resolver maps course identifiers seamlessly across three tiers:
1. Exact numeric OrgUnit ID (e.g. `1073361`).
2. Vault link metadata (extracts `ou=` or `/home/<id>` from `links.courselink` in `deadlines.json`).
3. Normalized course code matching (e.g., `CIS*4300` matches `cis4300`, `CIS4300`, or enrollment titles).

#### 4. Safe Reconciliation Engine
When running `d2l reconcile`:
* Analyzes dropbox due dates vs. vault `due_date` fields.
* Checks `mysubmissions` to confirm if files were submitted and auto-marks tasks as `completed`.
* Scrapes recent announcements and prepares a markdown digest for AI verification.
* Any mutation with `--apply` creates an atomic `.bak` backup copy (`deadlines.json.bak`) prior to modifying the file.

---

## 2. Installation & Setup (Fresh Machine)

### Prerequisites

1. **Rust Toolchain:** Rust 1.80+ with Cargo (Rust 2021 Edition).
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source "$HOME/.cargo/env"
   ```

2. **Google Chrome or Chromium:**
   Required for browser-based session interception and SSO authentication.
   The tool automatically searches the following locations:
   - `/usr/bin/google-chrome-stable`
   - `/usr/bin/google-chrome`
   - `/usr/bin/chromium`
   - `/usr/bin/chromium-browser`
   - Custom path defined by the `CHROME_BIN` environment variable.

   *On Arch Linux:*
   ```bash
   sudo pacman -S chromium
   # or for Google Chrome from AUR:
   # yay -S google-chrome
   ```

   *On Ubuntu / Debian:*
   ```bash
   sudo apt update && sudo apt install -y chromium-browser
   # or install google-chrome-stable via Google's .deb package
   ```

   *On Fedora:*
   ```bash
   sudo dnf install -y chromium
   ```

---

### Building from Source

```bash
# 1. Clone the repository
git clone https://github.com/LeeOs/d2l-class-manager.git
cd d2l-class-manager

# 2. Run test suite to verify system compatibility
cargo test

# 3. Build optimized release binary
cargo build --release

# 4. Install binary to your user PATH (~/.cargo/bin/d2l)
cargo install --path .
```

Verify the installation:
```bash
d2l --version
d2l --help
```

---

### System Configuration & Paths

`d2l-class-manager` conforms to the XDG Base Directory specification and organizes its data as follows:

| Path | Purpose |
|:---|:---|
| `~/.config/d2l-manager/token.json` | Cached D2L Bearer token, user claims, and expiration timestamp (`0600` permissions). |
| `~/.config/d2l-manager/browser_profile/` | Persistent Chromium user profile preserving institutional cookies & MFA session. |
| `~/.config/d2l-manager/courses.json` | Local cached catalog of enrolled courses. |
| `~/Documents/LeeOsVault/F26/` | Default academic vault containing course directories and `deadlines.json` files. |

---

### Environment Variables

You can configure `d2l` via environment variables in your `~/.bashrc` or `~/.zshrc`:

```bash
# Set your institution's D2L / CourseLink domain (default: courselink.uoguelph.ca)
export D2L_HOST="courselink.uoguelph.ca"

# Set the path to your course vault (where deadlines.json files reside)
export SCHOOL_DASHBOARD_VAULT="$HOME/Documents/LeeOsVault/F26"

# (Optional) Specify a custom Chrome or Chromium executable
export CHROME_BIN="/usr/bin/google-chrome-stable"

# (Optional) Bypass browser login by providing a static JWT Bearer token
# export D2L_TOKEN="eyJhbGciOi..."
```

---

## 3. Usage Guide

### Authentication (`login` & `status`)

#### First-Time Login (Interactive SSO / MFA)
Run `d2l login`. A browser window will open navigating to your institution's portal. Log in with your university credentials and complete any 2FA challenge (e.g., Duo Mobile). As soon as the page loads, `d2l` captures the Bearer JWT, caches your session in `browser_profile`, and closes the browser window:

```bash
d2l login
```

#### Subsequent Token Refreshes (Headless)
Tokens naturally expire after ~1 hour. When running any `d2l` command with an expired token, `d2l` automatically launches Chrome in the background in headless mode, uses your saved session cookies to grab a fresh token in ~1.5s, and proceeds with the command—**no user intervention required**.

You can also explicitly refresh headlessly or force a refresh:
```bash
# Headless refresh using existing cookies
d2l login --headless

# Force a new login even if the current token is still valid
d2l login --force

# Manually inject a token copied from browser DevTools
d2l login --token "eyJhbGciOi..."
```

#### Check Current Status
Verify your authentication status, user identity, and token lifetime:
```bash
d2l status
```
*Output:*
```text
🔒 D2L Authentication Active
User: Jane Doe (jdoe01)
User ID: 123456
Token Expires In: 54.2 min
```

For JSON output:
```bash
d2l status --json
```

---

### Inspecting Courses & Metadata (`courses`)

List all enrolled courses and their associated Brightspace OrgUnit IDs:

```bash
# List all active courses
d2l courses --active

# List all courses (including past semesters)
d2l courses

# Output as clean JSON
d2l courses --active --json
```

*Example Table Output:*
```text
┌────────────┬───────────┬──────────────────────────────────────────┬────────┐
│ OrgUnit ID │ Code      │ Course Name                              │ Active │
├────────────┼───────────┼──────────────────────────────────────────┼────────┤
│ 1073361    │ CIS*4300  │ Human Computer Interaction               │ Yes    │
│ 1074520    │ STAT*2050 │ Statistics II                            │ Yes    │
│ 1072911    │ CIS*3090  │ Parallel & Distributed Computing         │ Yes    │
└────────────┴───────────┴──────────────────────────────────────────┴────────┘
```

---

### Tracking Assignments & Submissions (`assignments`)

Query dropbox folders, instructions, due dates, total points, and submission statuses:

```bash
# List assignments for all active courses
d2l assignments

# Filter by a specific course (by code or OrgUnit ID)
d2l assignments -c CIS4300
d2l assignments -c 1073361

# Output as JSON
d2l assignments -c CIS4300 --json
```

*Example Table Output:*
```text
┌──────────┬───────────┬──────────────────────────┬──────────────────────────┬────────┬───────────┐
│ Course   │ Folder ID │ Assignment Name          │ Due Date                 │ Points │ Submitted │
├──────────┼───────────┼──────────────────────────┼──────────────────────────┼────────┼───────────┤
│ CIS*4300 │ 45291     │ Milestone 1: Needfinding │ 2026-10-09T23:59:00.000Z │ 100    │ ✅ Yes (1) │
│ CIS*4300 │ 45292     │ Milestone 2: Prototyping │ 2026-10-23T23:59:00.000Z │ 100    │ ❌ No      │
└──────────┴───────────┴──────────────────────────┴──────────────────────────┴────────┴───────────┘
```

---

### Course Announcements & News (`announcements`)

Fetch instructor announcements. HTML markup is automatically stripped and rendered into clean, readable Markdown:

```bash
# Fetch recent announcements across all courses
d2l announcements

# Fetch announcements for a specific course
d2l announcements -c CIS4300

# Only fetch announcements posted after a specific ISO datetime
d2l announcements -c CIS4300 --since 2026-10-01T00:00:00Z

# Output as JSON (useful for piping into LLMs)
d2l announcements -c CIS4300 --json
```

*Example Terminal Output:*
```text
--------------------------------------------------
📢 [CIS*4300] Milestone 1 Submission Grace Period Clarification
Posted: 2026-10-04T12:00:00.000Z

Hi everyone,

Please note that Milestone 1 dropbox remains open until 23:59 tonight. If your
team plans to use one of your 3 team grace days, please email the TA before the
deadline.
```

---

### Grades & Evaluation Breakdown (`grades`)

Retrieve gradebook items, points earned vs. total points, weighted percentages, and instructor feedback:

```bash
# View grades for all courses
d2l grades

# View grades for a specific course
d2l grades -c STAT2050

# Output as JSON
d2l grades -c STAT2050 --json
```

*Example Table Output:*
```text
┌───────────┬──────────────────────┬─────────────┬──────────────┬────────────────────────────┐
│ Course    │ Grade Item           │ Points      │ Weight %     │ Comments                   │
├───────────┼──────────────────────┼─────────────┼──────────────┼────────────────────────────┤
│ STAT*2050 │ Quiz 1: Probability  │ 19.0 / 20.0 │ 5.0 / 5.0%   │ Great work on Bayes rule!  │
│ STAT*2050 │ Midterm Examination  │ 84.5 / 100  │ 25.0 / 25.0% │ Solid performance in Sec B │
└───────────┴──────────────────────┴─────────────┴──────────────┴────────────────────────────┘
```

---

### Quizzes & Availability Windows (`quizzes`)

List upcoming quizzes, time limits, and availability windows:

```bash
# View all quizzes
d2l quizzes

# Filter by course
d2l quizzes -c STAT2050
```

---

### Cross-Course Calendar (`calendar`)

Fetch aggregated calendar events and schedule items across all enrolled courses:

```bash
# Fetch upcoming events for the next 14 days (default)
d2l calendar

# Look ahead 30 days
d2l calendar --days 30

# Output as JSON
d2l calendar --days 7 --json
```

---

### Discussion Forums & Readings (`posts`)

Explore course discussion forums, topic threads, and student/instructor messages:

```bash
# 1. List discussion forums for a course
d2l posts -c CIS4300

# 2. List topics within a specific forum ID
d2l posts -c CIS4300 --forum 88210

# 3. Read all posts inside a specific topic ID
d2l posts -c CIS4300 --forum 88210 --topic 194821
```

---

### Downloading Lecture Slides & Files (`download`)

Traverse the course Content Table of Contents (TOC) and download lecture PDFs, slides, and code attachments into your vault's `Materials` folder:

```bash
# Download course materials into $SCHOOL_DASHBOARD_VAULT/<COURSE>/Materials/
d2l download -c CIS4300

# Specify custom destination folder
d2l download -c CIS4300 --dest /home/lmb/Downloads/CIS4300_Slides
```

---

### Unified JSON State Export for AI (`dump`)

Generate a comprehensive, machine-readable JSON snapshot of user identity and complete course states (announcements, dropboxes, quizzes, and grades) in a single command. Ideal for piping into local LLMs or AI verification pipelines:

```bash
# Dump entire academic state across all courses
d2l dump > state_snapshot.json

# Dump a single course
d2l dump -c CIS4300 > cis4300_snapshot.json
```

---

### Vault Reconciliation Engine (`reconcile`)

Compare live CourseLink data with your local Obsidian vault's `deadlines.json` files:

```bash
# Dry run: check discrepancies across all vault courses
d2l reconcile

# Check discrepancies for a specific course
d2l reconcile -c CIS4300

# Apply reconciliation updates directly to deadlines.json
d2l reconcile -c CIS4300 --apply
```

#### What Reconciliation Detects & Fixes:
1. **Due Date Drift:** Detects when an instructor modifies a dropbox due date on D2L that differs from your vault's `due_date` by more than 60 minutes.
2. **Submitted Assignments:** Detects when a deliverable has a verified submission on D2L and updates `status` to `"completed"`.
3. **Missing Items:** Flags assignments present on D2L that are missing from your vault.
4. **Announcement Digest:** Collects recent announcements for semantic AI evaluation.
5. **Safe Mutation with Backups:** Whenever `--apply` writes changes, it creates an atomic `.bak` file (`deadlines.json.bak`) beforehand.

---

## 4. Integration with `School-Dashboard` & AI Layer

`d2l-class-manager` is designed as the headless data engine for [`School-Dashboard`](https://github.com/LeeOs/School-Dashboard) (`schoodash`), a terminal cockpit built with Ratatui.

### How They Work Together:
1. **Interactive Cockpit (`schoodash`):** Reads `deadlines.json` files and renders a keyboard-driven TUI (Radar, Timeline, Calendar, and AI Advisor).
2. **Automated Sync (`:sync` / `S`):** `schoodash` executes `d2l reconcile --apply` in the background.
3. **AI Verification Prompting:** The AI Advisor in `schoodash` reads `d2l dump --json` to analyze announcement text for informal deadline modifications, room changes, or syllabus updates, and proposes changes before patching your vault.

---

## 5. Troubleshooting & FAQ

### 1. `No Chrome/Chromium executable found`
**Cause:** Chrome or Chromium is not installed in standard `/usr/bin/` paths.  
**Fix:** Install Chromium or set the `CHROME_BIN` environment variable:
```bash
export CHROME_BIN="/path/to/your/chrome-or-chromium"
```

### 2. `Authentication error: Please run d2l login to authenticate`
**Cause:** No cached token exists or the persistent browser session cookies have expired.  
**Fix:** Run `d2l login` to open a headed browser window and re-authenticate via SSO / Duo 2FA.

### 3. `Rate limited (429). Retrying after Xs...`
**Cause:** D2L Valence API rate limit reached.  
**Behavior:** `d2l` automatically handles this by inspecting the `Retry-After` header and waiting before re-attempting the request (up to 3 retries).

### 4. Course code resolution failure (`Could not resolve course identifier`)
**Cause:** The course query did not match any active enrollment or vault mapping.  
**Fix:** Run `d2l courses --active` to view your enrolled course codes and OrgUnit IDs. You can always pass the exact numeric OrgUnit ID directly:
```bash
d2l assignments -c 1073361
```

---

## License

MIT License. See [LICENSE](LICENSE) for details.
