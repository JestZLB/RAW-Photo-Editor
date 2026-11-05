# ADR-013：命名快照（版本）

- 状态：已接受
- 日期：2026-11-05
- 关联：ADR-003 文档模型、ADR-004 预览与缓存

## 背景

撤销栈只在内存里，关闭应用即丢失（README 一直如实标注这一点）。用户需要的是「回到上周那一版」而不是「撤销 12 步」，两者对存储的要求完全不同：

- 历史是高频、短暂、可丢弃的；
- 版本是低频、长期、必须可靠的。

## 决策

1. 新增 SQLite 表 `project_versions`（`schema` user_version = 3）：`id / project_id / name / document_json / revision / thumbnail_path / created_at`，外键 `ON DELETE CASCADE`，删除照片时快照一并清理（连接已开启 `PRAGMA foreign_keys = ON`）。
2. 快照保存**完整文档 JSON**，而不是差量或操作日志。差量需要可重放的操作定义与顺序保证，收益是体积；但一次快照就是当前 `ProjectDocument` 的序列化结果，实现与恢复语义都无需额外推演，错误面最小。
3. 恢复时不覆盖项目身份：`id`、`collection_id`、`collection_name`、照片名与 `created_at` 一律取当前值。否则一个旧快照会把照片「搬回」旧照片集，甚至与已删除的项目 id 冲突。
4. 恢复本身产生一条可撤销历史（`恢复版本：<名称>`），revision 前进到 `max(当前, 快照) + 1`。所以误点恢复可以立刻撤销，预览缓存也不会撞上旧 revision。
5. 列表接口不返回 `document_json`：一份蒙版密集的文档是 MB 级，50 条快照全部读出会让一次列表请求变成几十 MB。缩略图优先取当前 revision 的渲染预览，退化为源图代理；路径失效时返回 `null`，由界面显示占位图标。
6. 每个项目最多保留 50 个快照，超出丢弃最旧的。快照是用户显式创建的，不需要像历史那样按字节做预算。
7. 名称长度 1..60 字符，空白会被裁剪；空名称返回 `INVALID_ARGUMENT`，由前端直接给出提示。

## 后果

- 优点：跨重启可回退；恢复可撤销；与现有 `DocumentSession` 的 `restore`/history 语义一致，没有第二套状态机。
- 代价：`project_versions` 会随快照数量线性增长（单条约等于文档 JSON 大小）；快照不包含 AI 任务队列、导出设置或撤销栈，恢复后这些保持当前值。

## 验证

- `named_versions_survive_a_restart_and_restore_as_undoable_steps`（`crates/app-core/tests/service.rs`）：
  1. 曝光 1.0 → 建版本「暖调」→ 改成 2.0；
  2. 恢复后曝光回到 1.0，历史末条为「恢复版本：暖调」；
  3. 撤销后回到 2.0；
  4. 用同一数据目录新建一个 `AppService`（等价于重启），仍能列出该版本且 id 一致。
- `src/stores/versions.test.ts`：加载列表、名称裁剪与空名拒绝、恢复后图层与选中项、删除后列表刷新。
