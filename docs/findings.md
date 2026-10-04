# D2L Brightspace Architecture & API Findings

This document summarizes the technical findings and architectural patterns extracted from [`clintonwrong53/d2l-cli`](https://github.com/clintonwrong53/d2l-cli), specifically focusing on authentication mechanisms, session management, and data retrieval patterns from D2L Brightspace.

---

## 1. Overview & Context

D2L Brightspace does not offer public self-service API client registration for students. Standard OAuth2 flows require institutional administrator approval and pre-registered client credentials. 

To overcome this constraint without modifying server-side configurations, `d2l-cli` acts as a read-only client that:
1. Automates authentication via a headless/headed browser session to capture temporary user Bearer JWTs.
2. Directly interacts with D2L's internal REST APIs (Valence API).
3. Connects to auxiliary third-party systems used by universities (such as SimpleSyllabus) via identifiers embedded in course metadata.

---

## 2. Authentication Architecture

### 2.1 The Authentication Challenge
* **No Direct Credentials Flow**: Universities use SAML/SSO with multi-factor authentication (e.g., Duo Security), making programmatic username/password submission unreliable and fragile.
* **Token Lifespan**: D2L web sessions issue short-lived JWT Bearer tokens that expire in approximately **1 hour** (`exp` claim).

### 2.2 Playwright Browser Interception Flow
The CLI implements token acquisition in `src/d2l/commands/auth_cmd.py` and `grab_token.py`:

```
User runs `d2l login`
   │
   ├──> Playwright launches Chromium with persistent context (`~/.d2l/browser_profile`)
   │
   ├──> Navigates to institutional portal (`https://<institution>.view.usg.edu/d2l/home`)
   │    └── User completes SSO / 2FA login (or session restores automatically from cookies)
   │
   ├──> `page.on("request")` listener monitors network traffic
   │    └── Inspects outgoing headers for `Authorization: Bearer eyJ...`
   │
   ├──> First valid Bearer token captured
   │
   ├──> Extracts JWT payload (base64 URL decode) to extract `exp`, `sub`, and `tenantid`
   │
   └──> Writes token + metadata to `~/.d2l/token.json`
```

#### Key Implementation Details:
1. **Persistent Browser Profile**:
   Using `p.chromium.launch_persistent_context(str(BROWSER_PROFILE), headless=headless)` stores cookies, local storage, and session tokens on disk. After the first login, subsequent runs often grab a token instantly without prompting the user.
2. **Sniffing Request Headers**:
   When the single-page application boots up, it makes background API calls with `Authorization: Bearer <token>`. The script catches the first request matching `Bearer eyJ`:
   ```python
   def on_request(request):
       nonlocal captured_token
       if captured_token:
           return
       auth = request.headers.get("authorization", "")
       if auth.startswith("Bearer eyJ"):
           captured_token = auth.removeprefix("Bearer ")
   ```
3. **Dependency-Free JWT Payload Extraction**:
   Instead of pulling in large crypto/JWT libraries, the token payload is split and base64-decoded directly with standard padding repair:
   ```python
   payload = captured_token.split(".")[1]
   payload += "=" * (4 - len(payload) % 4)
   claims = json.loads(base64.urlsafe_b64decode(payload))
   ```

### 2.3 Token Resolution Hierarchy
When any command executes, the token is resolved in the following priority (`src/d2l/auth.py`):
1. **`~/.d2l/token.json`**:
   - Reads `exp` timestamp.
   - If `exp > time.time()`, returns the token.
   - If expired, raises `TokenExpiredError("Token expired at ... Run: d2l login")`.
2. **Local `.env`**: Searches current working directory for `D2L_TOKEN=...`.
3. **Environment Variable**: `os.environ.get("D2L_TOKEN")`.

### 2.4 Session Construction & Header Spoofing
The HTTP client uses `requests.Session` configured to match browser requests to avoid WAF/CORS rejection:
```python
s = requests.Session()
s.headers.update({
    "Authorization": f"Bearer {token}",
    "Origin": LMS_HOST,
    "Referer": f"{LMS_HOST}/",
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 ...",
})
```

---

## 3. Data Retrieval Architecture (`D2LClient`)

All network requests are centralized in `src/d2l/client.py`. The client is strictly **read-only** (only issuing HTTP `GET`).

### 3.1 D2L Valence API Products & Versions
D2L namespaces endpoints by product suite and version:
* **Learning Platform (`lp`)** – Version `1.47`
  - Base: `/d2l/api/lp/1.47/`
  - Scope: Identity (`/users/whoami`), enrollments, user feeds.
* **Learning Environment (`le`)** – Version `1.80`
  - Base: `/d2l/api/le/1.80/`
  - Scope: Course content, grades, dropbox/assignments, quizzes, news, calendar.
* **Awards (`bas`)** – Version `2.2`
  - Base: `/d2l/api/bas/2.2/`
  - Scope: Badges and certificates.

### 3.2 Resilience and Rate Limiting
The client implements a 3-attempt retry loop on HTTP status `429 Too Many Requests`:
* Reads the `Retry-After` header (fallback default: 5 seconds).
* Sleeps for `retry_after` duration before retrying.
* Raises typed exceptions (`NotFoundError`, `ForbiddenError`, `RateLimitError`) for non-2xx status codes.

### 3.3 Pagination Mechanisms
D2L uses two distinct pagination patterns across its API:

#### 1. Bookmark Pagination (`paginate_bookmark`)
Used on major resource collections (such as `/enrollments/myenrollments/`).
* Structure returned by server:
  ```json
  {
    "PagingInfo": {
      "Bookmark": "eyJhbGciOi...",
      "HasMoreItems": true
    },
    "Items": [ ... ]
  }
  ```
* Strategy: The client begins with `bookmark=""` and continues fetching while `PagingInfo.HasMoreItems` is `true`, passing `?bookmark=<value>` in query parameters.

#### 2. Page-Number Pagination (`paginate_pages`)
Used on discussion post listings (`/discussions/forums/{forum_id}/topics/{topic_id}/posts/`).
* Query parameters: `?pageSize=50&pageNumber=N`.
* Strategy: Fetches starting at `pageNumber=1`, terminating when the returned array is empty or smaller than `pageSize`.

### 3.4 Course Name Resolution (`CourseResolver`)
To provide a smooth CLI experience where users enter strings like `"calc"` or `"data structures"` instead of numeric IDs:
1. Loads active courses once via `my_enrollments(active_only=True)` and caches them in memory.
2. Evaluates query against courses in a 4-tier priority ladder:
   - **Tier 1: Exact numeric ID match** (e.g. `"3824526"`).
   - **Tier 2: Exact course code match** (e.g. `"CS2720_S26"`).
   - **Tier 3: Substring match** in course name.
   - **Tier 4: Word overlap scoring** (jaccard-like set intersection between query tokens and course title words).
3. If multiple courses match at the same tier, it disambiguates by filtering for `Type.Name == "Course Offering"`, or prompts the user with candidates.

---

## 4. Specific Endpoint Inventory

| Function | Endpoint | Notes |
|:---|:---|:---|
| **Identity** | `GET /d2l/api/lp/1.47/users/whoami` | Returns user ID, unique username, and full name. |
| **Enrollments** | `GET /d2l/api/lp/1.47/enrollments/myenrollments/` | Filter parameters: `isActive=true`, `canAccess=true`, `sortBy=-StartDate`. |
| **Grades** | `GET /d2l/api/le/1.80/{org_id}/grades/values/myGradeValues/` | Returns individual assignment grades, weights, and points. |
| **Final Grades** | `GET /d2l/api/le/1.80/grades/final/values/myGradeValues/?orgUnitIdsCSV={csv}` | Bulk endpoint to pull final course grades across all courses in one request. |
| **Assignments** | `GET /d2l/api/le/1.80/{org_id}/dropbox/folders/` | Lists dropbox folders, instructions, due dates, and attachment IDs. |
| **Submissions** | `GET /d2l/api/le/1.80/{org_id}/dropbox/folders/{folder_id}/submissions/mysubmissions/` | User-submitted files and timestamps. |
| **Content TOC** | `GET /d2l/api/le/1.80/{org_id}/content/toc` | Complete Table of Contents tree for modules and topics. |
| **Module Details** | `GET /d2l/api/le/1.80/{org_id}/content/modules/{module_id}/structure/` | Deep structure of a specific module. |
| **Calendar Events**| `GET /d2l/api/le/1.80/calendar/events/myEvents/` | Can filter by `orgUnitIdsCSV`, `startDateTime`, and `endDateTime`. |
| **Due Items** | `GET /d2l/api/le/1.80/content/myItems/due/` | Upcoming due items across selected or all courses. |
| **Overdue Items**| `GET /d2l/api/le/1.80/overdueItems/myItems` | Current overdue items across courses. |
| **Quizzes** | `GET /d2l/api/le/1.80/{org_id}/quizzes/` | Available quizzes, attempt limits, and availability windows. |
| **Announcements**| `GET /d2l/api/le/1.80/{org_id}/news/?since={iso_date}` | Course announcements with optional date filtering. |
| **Discussions** | `GET /d2l/api/le/1.80/{org_id}/discussions/forums/` | Forums and topic threads. |

### 4.1 Binary File Downloads
Downloads (assignment prompt attachments and content topic files) stream raw responses:
* **Assignment Attachments**: `GET /d2l/api/le/1.80/{org_id}/dropbox/folders/{folder_id}/attachments/{file_id}`
* **Content Files**: `GET /d2l/api/le/1.80/{org_id}/content/topics/{topic_id}/file`
* **Filename Extraction**: The file name is parsed from the response's `Content-Disposition` header using regex:
  ```python
  match = re.search(r'filename[*]?=["\']?(?:UTF-8\'\')?([^"\';]+)', cd)
  ```

### 4.2 External Syllabus Integration (SimpleSyllabus)
Interestingly, syllabi are often hosted outside of D2L in an external service. In `src/d2l/commands/syllabus.py`:
1. D2L course codes often encode the university Course Reference Number (CRN) (e.g. `CO.430.CS3305.10931.20264` contains CRN `10931`).
2. The code extracts this CRN and calls the external SimpleSyllabus API:
   - `GET https://<institution>.simplesyllabus.com/api2/syllabus-search?search={crn}` &rarr; retrieves `syllabus_id`.
   - `GET https://<institution>.simplesyllabus.com/api2/doc-full-page-get?code={syllabus_id}` &rarr; retrieves full JSON document.
3. It strips HTML tags from component sections to produce readable plain text / markdown.

---

## 5. Architectural Recommendations for `d2l-class-manager`

1. **Decouple Auth from Business Logic**:
   - Provide a background or on-demand token refresher module using Playwright in headless mode.
   - Cache tokens securely with expiration checking before making API calls.
2. **Cache Course & Organization Data**:
   - Course names, OrgUnit IDs, and course codes rarely change mid-semester.
   - Caching enrollment lists locally minimizes unnecessary round-trips for fuzzy name resolution.
3. **Multi-Course Aggregation**:
   - Leverage batch query parameters like `orgUnitIdsCSV` for calendar, due dates, overdue items, and final grades instead of sequentially polling each course.
4. **Resilient Rate-Limit & Backoff Strategy**:
   - Maintain the `429` retry logic with `Retry-After` header inspection to prevent getting throttled during bulk syncs.
