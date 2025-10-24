#!/usr/bin/env python3
"""
Generates a 2-column Markdown snippet for weekly status:
- Last week's achievements (Status == Done)
- Plans for next week (Status == In Progress)

Data sources (in priority order):
  A) GitHub Projects v2 Status field (org-level or user-level project)
  B) Issue labels (DONE_LABELS / INPROGRESS_LABELS, comma-separated)

Environment:
  REPO                    (required) e.g. "input-output-hk/acropolis"
  # --- For Projects v2 (preferred) ---
  PROJECT_OWNER           (optional) e.g. "input-output-hk" (org login) or user login
  PROJECT_NUMBER          (optional) e.g. "7"
  STATUS_DONE_VALUE       (optional) defaults to "Done"
  STATUS_INPROGRESS_VALUE (optional) defaults to "In Progress"
  # --- Fallback via labels ---
  DONE_LABELS             (optional) comma-separated (default: "status: done,done")
  INPROGRESS_LABELS       (optional) comma-separated (default: "status: in progress,in progress")

Behavior:
  - "Last week" time window is the previous Mon–Sun in repo default timezone (UTC).
  - "Achievements" are items with Status == Done and (closed/merged/updated) in last week.
  - "Plans" are items currently Status == In Progress and updated within last 14 days.

Output:
  - Prints Markdown to stdout
  - If GITHUB_STEP_SUMMARY is set, also writes to it.
  - If OUTPUT_PATH is set, also writes file to that path.
"""

# Suppress urllib3 warnings early
import warnings
warnings.filterwarnings("ignore", message="urllib3 v2 only supports OpenSSL 1.1.1+")
warnings.filterwarnings("ignore", category=UserWarning, module="urllib3")

import os
import sys
import json
import datetime as dt
from typing import List, Dict, Optional, Tuple
import requests

GITHUB_API = "https://api.github.com/graphql"
REST_API   = "https://api.github.com"

def iso(d: dt.datetime) -> str:
    return d.replace(microsecond=0, tzinfo=dt.timezone.utc).isoformat().replace("+00:00", "Z")

def previous_monday(d: dt.date) -> dt.date:
    return d - dt.timedelta(days=(d.weekday()))  # Monday is 0

def last_week_window(today: dt.date) -> Tuple[dt.datetime, dt.datetime]:
    # Define "last week" as the Monday..Sunday immediately before the current week.
    this_monday = previous_monday(today)
    last_monday = this_monday - dt.timedelta(days=7)
    last_sunday = this_monday - dt.timedelta(seconds=1)
    start = dt.datetime.combine(last_monday, dt.time(0,0,0), tzinfo=dt.timezone.utc)
    end   = dt.datetime.combine(last_sunday, dt.time(23,59,59), tzinfo=dt.timezone.utc)
    return start, end

def gh_graphql(token: str, query: str, variables: dict) -> dict:
    r = requests.post(
        GITHUB_API,
        headers={"Authorization": f"bearer {token}", "Accept": "application/vnd.github+json"},
        json={"query": query, "variables": variables},
        timeout=60,
    )
    r.raise_for_status()
    data = r.json()
    if "errors" in data:
        raise RuntimeError(f"GitHub GraphQL errors: {data['errors']}")
    return data["data"]

def gh_rest(token: str, url: str, params: dict=None) -> dict:
    r = requests.get(
        url,
        headers={"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json"},
        params=params or {},
        timeout=60,
    )
    r.raise_for_status()
    return r.json()

def discover_projects(token: str, owner: str) -> List[dict]:
    """
    Discover available GitHub Projects v2 for the given owner (org or user).
    Returns list of {number, title} dicts.
    """
    query = """
    query($owner: String!) {
      organization(login: $owner) {
        projectsV2(first: 20) {
          nodes { number title }
        }
      }
      user(login: $owner) {
        projectsV2(first: 20) {
          nodes { number title }
        }
      }
    }
    """
    try:
        data = gh_graphql(token, query, {"owner": owner})
        projects = []
        
        # Try org projects first
        org_projects = data.get("organization", {})
        if org_projects and org_projects.get("projectsV2"):
            projects.extend(org_projects["projectsV2"]["nodes"] or [])
        
        # Try user projects
        user_projects = data.get("user", {})
        if user_projects and user_projects.get("projectsV2"):
            projects.extend(user_projects["projectsV2"]["nodes"] or [])
        
        return [p for p in projects if p]  # filter out nulls
    except Exception as e:
        print(f"[warn] Failed to discover projects for {owner}: {e}", file=sys.stderr)
        return []

def get_project_and_status_field(token: str, owner: str, number: int) -> Tuple[str, str, Dict[str,str]]:
    """
    Returns (projectId, statusFieldId, statusOptionsMap{name->optionId})
    """
    query = """
    query($owner: String!, $number: Int!) {
      organization(login: $owner) {
        projectV2(number: $number) {
          id
          fields(first: 50) {
            nodes {
              ... on ProjectV2FieldCommon {
                id
                name
                dataType
              }
              ... on ProjectV2SingleSelectField {
                id
                name
                dataType
                options { id name }
              }
            }
          }
        }
      }
      user(login: $owner) {
        projectV2(number: $number) {
          id
          fields(first: 50) {
            nodes {
              ... on ProjectV2FieldCommon {
                id
                name
                dataType
              }
              ... on ProjectV2SingleSelectField {
                id
                name
                dataType
                options { id name }
              }
            }
          }
        }
      }
    }
    """
    data = gh_graphql(token, query, {"owner": owner, "number": number})
    proj = data.get("organization", {}).get("projectV2") or data.get("user", {}).get("projectV2")
    if not proj:
        raise RuntimeError("Project not found (check PROJECT_OWNER/PROJECT_NUMBER).")
    project_id = proj["id"]

    status_field = None
    status_options = {}
    for f in proj["fields"]["nodes"]:
        if f and f.get("name") == "Status":
            status_field = f["id"]
            opts = f.get("options") or []
            status_options = {o["name"]: o["id"] for o in opts}
            break
    if not status_field:
        raise RuntimeError("Status field not found on the project.")
    return project_id, status_field, status_options

def items_by_status_from_project(token: str, owner: str, number: int,
                                 wanted_status_names: List[str],
                                 repo_fullname: str) -> Dict[str, List[dict]]:
    """
    Returns map {statusName: [ {title,url,number,updatedAt,closedAt,type} ]}
    filtered to the specified repo.
    """
    project_id, status_field_id, status_options = get_project_and_status_field(token, owner, number)
    wanted_option_ids = [status_options[n] for n in wanted_status_names if n in status_options]

    results = {n: [] for n in wanted_status_names}

    # Paginate through project items
    query = """
    query($projectId: ID!, $after: String) {
      node(id: $projectId) {
        ... on ProjectV2 {
          items(first: 100, after: $after) {
            pageInfo { hasNextPage endCursor }
            nodes {
              updatedAt
              content {
                __typename
                ... on Issue {
                  number
                  title
                  url
                  repository { nameWithOwner }
                  closedAt
                }
                ... on PullRequest {
                  number
                  title
                  url
                  repository { nameWithOwner }
                  mergedAt
                  closedAt
                }
              }
              fieldValues(first: 20) {
                nodes {
                  __typename
                  ... on ProjectV2ItemFieldSingleSelectValue {
                    field { ... on ProjectV2SingleSelectField { id name } }
                    optionId
                  }
                }
              }
            }
          }
        }
      }
    }
    """
    after = None
    while True:
        data = gh_graphql(token, query, {"projectId": project_id, "after": after})
        items = data["node"]["items"]["nodes"]
        for it in items:
            # find the Status value (optionId)
            option_id = None
            for fv in it["fieldValues"]["nodes"]:
                if fv.get("__typename") == "ProjectV2ItemFieldSingleSelectValue" and \
                   fv.get("field", {}).get("id") == status_field_id:
                    option_id = fv.get("optionId")
                    break
            if option_id not in wanted_option_ids:
                continue

            content = it.get("content") or {}
            typename = content.get("__typename")
            if typename not in ("Issue", "PullRequest"):
                continue
            if content["repository"]["nameWithOwner"].lower() != repo_fullname.lower():
                continue

            closedAt = content.get("closedAt")
            mergedAt = content.get("mergedAt")
            updatedAt = it["updatedAt"]

            results_key = None
            for name, oid in status_options.items():
                if oid == option_id:
                    results_key = name
                    break
            if not results_key:
                continue

            results[results_key].append({
                "type": typename,
                "title": content["title"],
                "url": content["url"],
                "number": content["number"],
                "closedAt": closedAt,
                "mergedAt": mergedAt,
                "updatedAt": updatedAt,
            })

        pi = data["node"]["items"]["pageInfo"]
        if not pi["hasNextPage"]:
            break
        after = pi["endCursor"]

    return results

def search_by_labels(token: str, repo: str, labels: List[str]) -> List[dict]:
    """
    Simple REST search for issues with any of the labels.
    """
    items = []
    for lab in labels:
        page = 1
        while True:
            res = gh_rest(token, f"{REST_API}/repos/{repo}/issues",
                          params={"state": "all", "labels": lab, "per_page": 100, "page": page})
            if not res:
                break
            for it in res:
                # Skip pull-request-only stubs unless desired; GitHub REST issues may include PRs
                title = it["title"]
                url = it["html_url"]
                num = it["number"]
                closedAt = it.get("closed_at")
                updatedAt = it.get("updated_at")
                is_pr = "pull_request" in it
                items.append({
                    "type": "PullRequest" if is_pr else "Issue",
                    "title": title, "url": url, "number": num,
                    "closedAt": closedAt, "mergedAt": None, "updatedAt": updatedAt
                })
            if len(res) < 100:
                break
            page += 1
    # de-dup by number
    seen = {}
    for x in items:
        seen[x["number"]] = x
    return list(seen.values())

def get_recent_issues_by_state(token: str, repo: str, state: str = "all", days: int = 7) -> List[dict]:
    """
    Fetch recent issues/PRs from the repo by state (open/closed/all).
    More targeted approach when labels aren't well organized.
    """
    items = []
    page = 1
    
    # Calculate the date threshold
    since_date = dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=days)
    since_iso = since_date.isoformat().replace('+00:00', 'Z')
    
    while True:
        params = {
            "state": state,
            "per_page": 100,
            "page": page,
            "sort": "updated",
            "direction": "desc",
            "since": since_iso
        }
        
        res = gh_rest(token, f"{REST_API}/repos/{repo}/issues", params=params)
        if not res:
            break
            
        for it in res:
            title = it["title"]
            url = it["html_url"]
            num = it["number"]
            closedAt = it.get("closed_at")
            updatedAt = it.get("updated_at")
            createdAt = it.get("created_at")
            state_val = it.get("state")
            is_pr = "pull_request" in it
            
            items.append({
                "type": "PullRequest" if is_pr else "Issue",
                "title": title, 
                "url": url, 
                "number": num,
                "closedAt": closedAt, 
                "mergedAt": None, 
                "updatedAt": updatedAt,
                "createdAt": createdAt,
                "state": state_val
            })
            
        if len(res) < 100:
            break
        page += 1
    
    return items

def format_markdown(done_items: List[dict], inprog_items: List[dict]) -> str:
    def fmt(items: List[dict]) -> str:
        if not items:
            return "_(none)_"
        # sort by updatedAt desc
        items = sorted(items, key=lambda x: x.get("updatedAt") or "", reverse=True)
        lines = []
        for it in items:
            t = "PR" if it["type"] == "PullRequest" else "Issue"
            lines.append(f"- [{t} #{it['number']}]({it['url']}) — {it['title']}")
        return "\n".join(lines)

    left  = fmt(done_items)
    right = fmt(inprog_items)

    # Two separate sections for easy copying to Confluence
    md = []
    md.append("## Last week's achievements")
    md.append("")
    md.append(left)
    md.append("")
    md.append("## Plans for next week")  
    md.append("")
    md.append(right)

    return "\n".join(md)

def main():
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    repo  = os.environ.get("REPO")
    if not token or not repo:
        print("Missing GH_TOKEN/GITHUB_TOKEN or REPO (e.g., 'input-output-hk/acropolis')", file=sys.stderr)
        sys.exit(2)

    # time windows
    today = dt.datetime.now(dt.timezone.utc).date()
    last_start, last_end = last_week_window(today)

    status_done_val  = os.environ.get("STATUS_DONE_VALUE", "Done")
    status_ip_val    = os.environ.get("STATUS_INPROGRESS_VALUE", "In Progress")

    done_items: List[dict] = []
    inprog_items: List[dict] = []

    proj_owner   = os.environ.get("PROJECT_OWNER")
    proj_number  = os.environ.get("PROJECT_NUMBER")

    # Auto-discover projects if not specified
    if not proj_owner or not proj_number:
        # Try the repo owner first
        repo_owner = repo.split('/')[0]
        print(f"[info] No project specified, discovering projects for {repo_owner}...", file=sys.stderr)
        projects = discover_projects(token, repo_owner)
        
        if projects:
            print(f"[info] Found {len(projects)} projects:", file=sys.stderr)
            for p in projects:
                print(f"  - Project {p['number']}: {p['title']}", file=sys.stderr)
            
            # Use the first project as default
            if not proj_owner:
                proj_owner = repo_owner
            if not proj_number:
                proj_number = str(projects[0]['number'])
                print(f"[info] Auto-selecting project {proj_number}: {projects[0]['title']}", file=sys.stderr)
        else:
            print(f"[info] No projects found for {repo_owner}", file=sys.stderr)

    try_projects = bool(proj_owner and proj_number)

    if try_projects:
        try:
            print(f"[info] Attempting to use GitHub Project: owner={proj_owner}, number={proj_number}", file=sys.stderr)
            buckets = items_by_status_from_project(
                token, proj_owner, int(proj_number),
                [status_done_val, status_ip_val],
                repo_fullname=repo
            )
            done_items = buckets.get(status_done_val, [])
            inprog_items = buckets.get(status_ip_val, [])
            print(f"[info] Found {len(done_items)} done items, {len(inprog_items)} in-progress items from project", file=sys.stderr)
        except Exception as e:
            print(f"[warn] Project v2 lookup failed, falling back to state-based filtering: {e}", file=sys.stderr)
            try_projects = False

    if not try_projects:
        # Try label-based approach first
        done_labels = os.environ.get("DONE_LABELS", "").split(",")
        ip_labels   = os.environ.get("INPROGRESS_LABELS", "").split(",")
        
        # Filter out empty labels
        done_labels = [s.strip() for s in done_labels if s.strip()]  
        ip_labels   = [s.strip() for s in ip_labels if s.strip()]
        
        if done_labels or ip_labels:
            # Use label-based approach if labels are specified
            done_items  = search_by_labels(token, repo, done_labels) if done_labels else []
            inprog_items= search_by_labels(token, repo, ip_labels) if ip_labels else []
        else:
            # Fallback: use state-based approach 
            print("[info] No specific labels configured, using state-based filtering", file=sys.stderr)
            
            # Get all recent issues from the last 14 days for broader context
            all_recent = get_recent_issues_by_state(token, repo, state="all", days=14)
            
            # Split into done (closed) and in-progress (open) items
            done_items = [it for it in all_recent if it.get("state") == "closed"]
            inprog_items = [it for it in all_recent if it.get("state") == "open"]

    # Filter by time windows
    def closed_last_week(it: dict) -> bool:
        # For achievements: closed/merged in the last week
        for k in ("mergedAt", "closedAt"):
            v = it.get(k)
            if not v:
                continue
            t = dt.datetime.fromisoformat(v.replace("Z","+00:00"))
            if last_start <= t <= last_end:
                return True
        return False

    def updated_recent(it: dict, days=7) -> bool:
        # For in-progress: updated in the last week
        v = it.get("updatedAt")
        if not v:
            return False
        t = dt.datetime.fromisoformat(v.replace("Z","+00:00"))
        return (dt.datetime.now(dt.timezone.utc) - t).days <= days

    # Achievements: items that were closed in the last week
    achievements = [it for it in done_items if closed_last_week(it)]
    
    # Plans: open items that were updated in the last week (showing active work)
    plans = [it for it in inprog_items if it.get("state") == "open" and updated_recent(it, days=7)]

    md = format_markdown(achievements, plans)

    # Print and optionally write outputs
    print(md)
    step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if step_summary:
        with open(step_summary, "a", encoding="utf-8") as f:
            f.write("\n\n## Weekly Status (auto-generated)\n")
            f.write(md)
            f.write("\n")

    out_path = os.environ.get("OUTPUT_PATH")
    if out_path:
        os.makedirs(os.path.dirname(out_path), exist_ok=True)
        with open(out_path, "w", encoding="utf-8") as f:
            f.write(md)

if __name__ == "__main__":
    main()
