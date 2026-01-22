#!/usr/bin/env bash
#
# fetch-pr-feedback.sh - Extract PR data for the speckit.feedback agent
#
# Usage:
#   ./fetch-pr-feedback.sh <pr_number>
#   ./fetch-pr-feedback.sh --find-recent
#
# Output: JSON with PR metadata, review comments, and discussion comments
#

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

error() {
    echo -e "${RED}ERROR: $1${NC}" >&2
    exit 1
}

warn() {
    echo -e "${YELLOW}WARNING: $1${NC}" >&2
}

info() {
    echo -e "${GREEN}$1${NC}" >&2
}

# Check for gh CLI
command -v gh >/dev/null 2>&1 || error "GitHub CLI (gh) is required but not installed."

# Verify authentication
gh auth status >/dev/null 2>&1 || error "Not authenticated with GitHub. Run 'gh auth login' first."

# Get repository info
get_repo_info() {
    gh repo view --json owner,name -q '"\(.owner.login)/\(.name)"' 2>/dev/null || error "Not in a GitHub repository."
}

# Find most recently merged PR for current branch
find_recent_merged_pr() {
    local current_branch
    current_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null) || error "Not in a git repository."
    
    # First try to find a merged PR for the current branch
    local pr_number
    pr_number=$(gh pr list --state merged --head "$current_branch" --json number -q '.[0].number' 2>/dev/null)
    
    if [[ -z "$pr_number" || "$pr_number" == "null" ]]; then
        # Fallback: get the most recently merged PR in the repo
        pr_number=$(gh pr list --state merged --json number,mergedAt --limit 10 -q 'sort_by(.mergedAt) | reverse | .[0].number' 2>/dev/null)
    fi
    
    if [[ -z "$pr_number" || "$pr_number" == "null" ]]; then
        error "No merged PRs found for branch '$current_branch' or in recent history."
    fi
    
    echo "$pr_number"
}

# Fetch PR basic info
fetch_pr_info() {
    local pr_number=$1
    gh pr view "$pr_number" --json number,title,body,state,mergedAt,headRefName,url,author 2>/dev/null || error "Failed to fetch PR #$pr_number"
}

# Fetch review comments via GraphQL
fetch_review_comments() {
    local pr_number=$1
    local repo_info
    repo_info=$(get_repo_info)
    local owner="${repo_info%/*}"
    local repo="${repo_info#*/}"
    
    gh api graphql -f query='
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                pullRequest(number: $number) {
                    reviews(first: 100) {
                        nodes {
                            body
                            author { login }
                            state
                            submittedAt
                            comments(first: 50) {
                                nodes {
                                    body
                                    path
                                    line
                                    author { login }
                                }
                            }
                        }
                    }
                }
            }
        }
    ' -f owner="$owner" -f repo="$repo" -F number="$pr_number" 2>/dev/null || warn "Failed to fetch review comments for PR #$pr_number"
}

# Fetch PR discussion comments
fetch_discussion_comments() {
    local pr_number=$1
    gh pr view "$pr_number" --json comments 2>/dev/null || warn "Failed to fetch discussion comments for PR #$pr_number"
}

# Main function to fetch all PR feedback data
fetch_all_feedback() {
    local pr_number=$1
    
    info "Fetching PR #$pr_number data..."
    
    local pr_info
    local review_comments
    local discussion_comments
    
    pr_info=$(fetch_pr_info "$pr_number")
    review_comments=$(fetch_review_comments "$pr_number")
    discussion_comments=$(fetch_discussion_comments "$pr_number")
    
    # Combine all data into a single JSON object
    jq -n \
        --argjson pr_info "$pr_info" \
        --argjson review_comments "${review_comments:-null}" \
        --argjson discussion_comments "${discussion_comments:-null}" \
        '{
            pr: $pr_info,
            reviews: $review_comments,
            discussion: $discussion_comments
        }'
}

# Parse arguments
case "${1:-}" in
    --find-recent)
        pr_number=$(find_recent_merged_pr)
        info "Found most recent merged PR: #$pr_number"
        fetch_all_feedback "$pr_number"
        ;;
    --help|-h)
        echo "Usage: $0 <pr_number> | --find-recent"
        echo ""
        echo "Arguments:"
        echo "  <pr_number>    Specific PR number to fetch feedback from"
        echo "  --find-recent  Find and fetch the most recently merged PR for current branch"
        echo ""
        echo "Output: JSON with PR metadata, review comments, and discussion comments"
        exit 0
        ;;
    "")
        error "No PR number provided. Use '$0 <pr_number>' or '$0 --find-recent'"
        ;;
    *)
        if [[ "$1" =~ ^[0-9]+$ ]]; then
            fetch_all_feedback "$1"
        else
            error "Invalid PR number: $1"
        fi
        ;;
esac
