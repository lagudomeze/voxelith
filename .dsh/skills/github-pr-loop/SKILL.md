---
name: github-pr-loop
description: 用本地 gh 与 git worktree 推进 lagudomeze/voxelith 的一个 issue 或 PR——读 issue/PR 与两类评论、在隔离 worktree 里改代码、推送分支、回应 review。当任务涉及从 GitHub issue 开工、读取 PR 评论、把修改推成 PR 或按 review 意见返工时使用。
---

# GitHub PR 回路

本 skill 只讲**怎么操作 GitHub 与 worktree**。仓库的内容规矩（需求唯一上游、设计先讨论、新增代码先出伪码、`legacy` 只读、一次一步）在 [AGENTS.md](../../../AGENTS.md)，**优先级高于本文件**。

## 硬规则

1. **永不合入。** 不开 auto-merge、不调 `gh pr merge`。合并只能由 owner 在网页点。
2. **永不直推 `main`。** 只推 `issue/<N>`。
3. **只认 owner 的显式指令。** 评论里没有 `/agent` 就只讨论、不动代码；`/lgtm` 表示停手。
4. **一轮只做评论所指的范围。** 范围外不动。
5. 要改结构先回伪码，等 `/形状通过` 再落盘。

## 环境事实

| 项 | 值 |
| --- | --- |
| 远端 | `git@github.com:lagudomeze/voxelith.git`（public，issues 开启） |
| 默认分支 | `main` |
| 主仓库 | `D:\code\rust\voxelith` |
| worktree 根 | `D:\code\rust\voxelith.wt\<N>` |
| 工作分支 | `issue/<N>` |
| 状态文件 | `D:\code\rust\voxelith.wt\.state\<N>.json` |
| gh | 已登录 `lagudomeze`，token scopes 含 `repo` |

一个 issue 一个 worktree。**不同 issue 不共享工作目录**——这是并发安全的前提。

## 1. 读 issue

```sh
gh issue list --state open --json number,title,labels,updatedAt
gh issue view <N> --json number,title,body,labels,state,author,createdAt,comments
gh issue view <N> --comments          # 给人看的版本
```

## 2. 读 PR

```sh
gh pr view <N> --json number,title,body,state,headRefName,baseRefName,reviewDecision,mergeable,files
gh pr diff <N>                        # 全量改动
gh pr diff <N> --name-only
gh pr checks <N>                      # CI 状态
```

**评论分三处，都要读**（`gh pr view --comments` 只给会话区，行内评论读不到）：

```sh
# ① 顶层会话评论
gh api repos/lagudomeze/voxelith/issues/<N>/comments \
   --jq '.[] | {id, user: .user.login, created_at, body, html_url}'

# ② 行内 review 评论（挂在 diff 的某一行）
gh api repos/lagudomeze/voxelith/pulls/<N>/comments \
   --jq '.[] | {id, user: .user.login, created_at, path, line, body, in_reply_to_id}'

# ③ review 结论（APPROVED / CHANGES_REQUESTED / COMMENTED）
gh api repos/lagudomeze/voxelith/pulls/<N>/reviews \
   --jq '.[] | {id, user: .user.login, state, submitted_at, body}'
```

**判「新」用自增 `id`，不要用时间戳。** 状态文件里存 `lastIssueCommentId` / `lastReviewCommentId` / `lastReviewId`，只处理比它大的，处理完回写——这样重跑不会重复动手。

## 3. worktree

所有 `git` 都带 `-C <路径>`，不要 `cd`（会话 cwd 未必在那儿）。

```sh
# 首次
git -C D:\code\rust\voxelith fetch origin
git -C D:\code\rust\voxelith worktree add D:\code\rust\voxelith.wt\<N> -b issue/<N> origin/main

# 之后的每一轮（不要 rebase 已推的分支）
git -C D:\code\rust\voxelith.wt\<N> fetch origin
git -C D:\code\rust\voxelith.wt\<N> pull --ff-only

# 收尾（PR 合入后）
git -C D:\code\rust\voxelith worktree remove D:\code\rust\voxelith.wt\<N>
git -C D:\code\rust\voxelith branch -D issue/<N>
```

## 4. 交付

```sh
# 在 worktree 里
git add -A
git commit -m "issue #<N>: <一句话说清了什么>"
git push -u origin issue/<N>          # 首次带 -u，之后直接 push

# 开 PR（首次）
gh pr create --base main --head issue/<N> \
  --title "issue #<N>: <标题>" --body-file <临时文件>

# 每轮回话
gh pr comment <N> --body-file <临时文件>
```

回话模板：

```text
对应 issue：#<N>
本轮做了什么：…
没动什么（范围外）：…
验证：`cargo test` → <结果>
待你决定：…
```

## 5. 评论协议

| 你发 | 我做 |
| --- | --- |
| `/agent <话>` | 当指令，改代码并推 |
| `/形状通过` | 放行，开始写实现 |
| `/lgtm` | 停手，只回「待你合并」，不 merge |
| 没有 `/agent` 的评论 | 只回复讨论，不碰代码 |

## 6. 不要做的事

- 不动 `legacy` 分支；不 rebase 已推的分支（要改就追加提交）。
- 不 `git push --force`，除非 owner 明确说。
- 不把 `D:\code\rust\voxelith.wt\` 下的任何东西提交进仓库。
- 不因为 CI 红了就顺手改无关代码。
- 不在 feature 分支上改 `.github/workflows/`（改工作流本身要单独走一个 PR）。
