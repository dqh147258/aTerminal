- [x] 全屏设置、蓝色样式、工作空间入口
- [x] 配置编辑、目录/能力、绑定/读取、冲突可靠性
- [x] MCP/Skills 完整编辑、资源上传
- [x] 独立 Global、Session 隔离与旧归档移除
- [x] 构建、关键检查、功能矩阵与交付

证据：独立 check-ios-settings.py 9/9 PASS；最终 xcodebuild Debug x86_64 Simulator BUILD SUCCEEDED（无Swift警告）；Release swiftc -typecheck exit 0；git diff --check通过。Review R1–R8已处理，正常关闭/离线历史在统一列表可达。未运行模拟器/RPC，交协调者验收。

- [x] 追加正常Agent缓存正文异步搜索、scope/query屏障及fixture
- [x] 追加搜索确定性检查、x86_64构建与[未Review]提交
- [x] 同批修复binding.reasoning兼容性，直接生产回归检查
