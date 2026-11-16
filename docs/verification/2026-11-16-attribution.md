# Commit attribution correction — 2026-11-16

The owner explicitly authorized rewriting both published branches to attribute
all commits to `gatgserv`. All 12 author and committer identities now use that
account's GitHub noreply identity. GitHub API readback verified account mapping
for all 12 commits and both default-branch commits. There are no added assistant
co-author trailers. Repository-local Git configuration and AGENTS.md require the
same identity for future commits.

The update was atomic with exact old-head force-with-lease checks. Every original
and rewritten commit has the identical tree, message and timestamps; only author,
committer and parent metadata changed. Uncommitted development files were checked
byte-for-byte and preserved. PR #2 remains open against main. This was a bounded
owner-approved exception; it does not authorize subsequent force-pushes or main
updates.

Earlier verification reports retain the exact original tested revisions. Use
this mapping to find the equivalent content after the attribution correction;
new code still requires fresh verification. Previous CI runs certify their
recorded original revisions, not subsequent changes.

| Original revision | Same content, corrected attribution |
| --- | --- |
| `55641f8d81d9085322df4653e0a9e09b355e603b` | `b0a3a53060e11bdca1fd712b6a1137002ba4e9ae` |
| `63db389be35d1fda60630d08d6a33e23be415a00` | `9f3fd0aaeb148652193b04d6bc6bf0dd9e39914b` |
| `633d5c6963ed4a08e658448ebe5f9f8b5cb0fd84` | `d311317480822c241fb5b386cfdf82935bf2f0b0` |
| `7c19d6d18f74893930d312937c101aaa9033daef` | `af6b3a9a7982657b69647702e2ea3faaf1c83558` |
| `9864ac482ef3c30e8a032af9a5f41944dae9fe00` | `5c5a6293b415a30aa7ae90dc6ffcc3aa54d8871e` |
| `b17a81f512a791959c2c0092378932e893fa4a58` | `c1ac2ac408bac16084e4c824673535b6147d0e85` |
| `fbcf75a0877f5a7cb1c5a3c803432638da6009aa` | `68f2990ad434fe8df8dc409c5a7e36dd74da6de6` |
| `7f177cacb7a16c4243faa31b6b8eb3ebc163fed9` | `7a1fd2d85c2c00f86430b29a33f9fc282f7d7f8d` |
| `12d8e33268b7bb798e721dea8261eb6f863464da` | `f425ddb0ce4bebac94384239cde0aa5c73de866a` |
| `43ecdc5219c1330dc5def16ac12e6a942a5acfcf` | `57ccbc14f75a4dfc35315c84ed783071506c8eaf` |
| `4b0c9fd40ee39b40761e7453caf8d2b8841f4791` | `5c0baff2a81e06228135de044ce44da70956d78e` |
| `cd5406899229f8e5342e67b1e79535914fb6ea9a` | `1b7d238ed2cc609e362224e95d9f0dcf362609a3` |
