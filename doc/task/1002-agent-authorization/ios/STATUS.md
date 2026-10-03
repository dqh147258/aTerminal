# iOS 授权交付

状态：Completed（iOS 分工）。有效批准来自父 PLAN/CONTRACT 与用户无需计划Review的授权。

产品源码 `d4f9021` 完成 Desktop 实际对话权限、CAS/full跨Run、once/always/deny、问答与持久pending恢复、规则查看/撤销、完整脱敏参数/长详情fingerprint ACK。稳定ChatPanel持有question/details sheet，AssistantModel按scope/pending管理draft，失败或手动关闭保留，服务端确认消费后清除；账号/scope/连接变化立即dismiss并拒绝旧编辑/提交。旧allowInput草稿不升级full或新授权。

High 必要检查：生产Foundation授权/草稿生命周期测试通过；最新Xcode build-for-testing成功；聚焦keyboard1/1；最终9个本地原生UI回归9/9；完整真实加密UI/RPC1/1并独立精确marker核验通过。详见 [VERIFICATION.json](VERIFICATION.json) 和 [HANDOFF.md](HANDOFF.md)，其中旧失败证据保留为历史。

真实RPC：once1行、native always3行（含精确复用与重授）、full2行（跨Run），long6651字节精确一致，deny/full-off无文件；真实问答consumed并返回iOS custom answer。所有操作通过实际可见UI，Root独立私有fixture与专用SE，Xcode jobs2/parallel NO。设备readonly/legacy UI和scope隔离由本地UI/Foundation覆盖；不冒充物理设备或另一真实只读iOS设备测试。

系统窗口已释放，无未决iOS实现或必要检查。最后runner/证据提交交root整合main并关闭其私有服务；不修改用户设备/账号/生产供应商，不删worktree或branch，不自行关闭终端。
