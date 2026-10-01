# 永久20页参考与XCTest截图路由

已逐页查看 `doc/task/0930-app-settings-redesign/reference/01-terminal.png` 至 `20-global-chat.png`。参考页的旧手机归档忽略，品牌aTerminal/服务器摘要编辑按UI_REFERENCE用户例外；系统状态栏/Home条不重复绘制。下面capture复用已有业务测试，只有fixture数据，运行后的附件才是iOS截图；本任务未操作sim。

| 参考页 | capture名称 | 现有测试 |
| --- | --- | --- |
| 01 terminal | fixture-01-terminal | testUnifiedDrawerAndIndependentGlobalEntrypoint |
| 02 settings | fixture-02-settings-home / restored | testSettingsSectionsReturnAndCancel / testSettingsPersistenceAndReset |
| 03 llm | fixture-03-llm | testCurrentBindingCanReturnToInheritanceAndReadingSaveReturnsHome |
| 04 provider | fixture-04-provider-new / edit / failed-draft / saved | testAzureConditionAndValidationPreserveDraft等 |
| 05 Azure | fixture-05-provider-azure | testAzureConditionAndValidationPreserveDraft |
| 06 model | fixture-06-model | testCatalogPaginationQueryResetAndSelectionReturnsToModel |
| 07 advanced | fixture-07-model-advanced | 同上 |
| 08 catalog | fixture-08-model-catalog | 同上 |
| 09 page2 | fixture-09-model-catalog-page-two | 同上 |
| 10 bindings | fixture-10-default-bindings | testCurrentBindingCanReturnToInheritanceAndReadingSaveReturnsHome |
| 11 reading | fixture-11-terminal-reading | testReadingValidationAndExtensionCancelRoutes |
| 12 MCP | fixture-12-mcp-list / detail | testFixtureMcpImportDeleteConfirmationAndSkillEdit |
| 13 import | fixture-13-mcp-import | 同上 |
| 14 Skills | fixture-14-skills-list / skill-detail | 同上 |
| 15 install/edit | fixture-15-skill-install / skill-editor | testReadingValidationAndExtensionCancelRoutes / testFixtureMcpImportDeleteConfirmationAndSkillEdit |
| 16 account | fixture-16-account-devices | testSettingsSectionsReturnAndCancel |
| 17 workspace | fixture-17-unified-workspace / fixture-closed-read-only / fixture-offline-read-only | testUnifiedDrawerAndIndependentGlobalEntrypoint / testClosedAndOfflineHistoryRemainReachableAndReadOnly |
| 18 Session | fixture-18-session-chat | testSessionAndGlobalDraftsStaySeparateAndReturnToList |
| 19 Global list | fixture-19-independent-global-list | testUnifiedDrawerAndIndependentGlobalEntrypoint |
| 20 Global chat | fixture-20-global-draft-isolation | testSessionAndGlobalDraftsStaySeparateAndReturnToList |

大字体/键盘/横屏补充：fixture-04-provider-large-text-keyboard / fixture-04-provider-landscape-keyboard。截图含键盘时记录具体状态，不以该图替代同页无键盘/小屏验收。与永久参考比较布局/层级，fixture字段/演示文字不作为真实配置证据。真实RPC不capture含用户配置内容的界面。
