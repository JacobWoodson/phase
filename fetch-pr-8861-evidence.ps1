# Scratch evidence collector for PR #8861 (read-only: no mutations).
# Run in YOUR shell (where `gh auth status` passes), then tell the agent to resume:
#   powershell -File D:\Code\phase\.claude\worktrees\phase-pr-8861-339597\fetch-pr-8861-evidence.ps1
$ErrorActionPreference = 'Stop'
$OutDir = 'D:\Code\phase\.claude\worktrees\phase-pr-8861-339597\.pr-evidence'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

gh pr view 8861 --repo phase-rs/phase --json number,title,state,author,assignees,headRefName,headRepository,baseRefName,isCrossRepository,mergeable,mergeStateStatus,reviewDecision,url,headRefOid,labels,autoMergeRequest,createdAt,updatedAt > "$OutDir\pr-view.json"

gh pr checks 8861 --repo phase-rs/phase --json name,state,bucket,workflow,event,link,description,startedAt,completedAt > "$OutDir\pr-checks.json"

$query = @'
query($owner:String!,$name:String!,$pr:Int!,$reviewsCursor:String,$commentsCursor:String,$threadsCursor:String) {
  repository(owner:$owner, name:$name) {
    pullRequest(number:$pr) {
      reviewDecision
      reviews(first:100, after:$reviewsCursor) {
        pageInfo { hasNextPage endCursor }
        nodes { author{login} body state submittedAt url }
      }
      comments(first:100, after:$commentsCursor) {
        pageInfo { hasNextPage endCursor }
        nodes { author{login} body url createdAt }
      }
      reviewThreads(first:100, after:$threadsCursor) {
        pageInfo { hasNextPage endCursor }
        nodes { isResolved isOutdated path line
                comments(first:50) { pageInfo { hasNextPage endCursor }
                                     nodes { author{login} body url createdAt } } }
      }
    }
  }
}
'@
gh api graphql -F owner=phase-rs -F name=phase -F pr=8861 -f query=$query --jq '.data.repository.pullRequest' > "$OutDir\pr-feedback.json"

echo "Wrote: $OutDir\pr-view.json, pr-checks.json, pr-feedback.json"
