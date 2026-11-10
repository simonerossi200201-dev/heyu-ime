# 版权与开源合规说明 (`docs/LICENSING.md`)

本文档完整记录「鹤羽音形 (HEYU YinXing)」仓库内所有代码、数据与工具文件的来源与许可边界，说明**个人私有词表**与**公开可分发开源词表**的隔离机制，并提供公开上传至 GitHub 的一键合规打包流程。

---

## 一、全仓库文件来源与许可分类总表

| 路径 / 模块 | 分类 | 来源说明 | 许可证 | 是否包含在公开版 (`public` 分支 / 发布包) |
| :--- | :--- | :--- | :--- | :--- |
| `src/*`, `tsf/*`, `voice/*`, `heyu_studio.py` | **原创代码** | 本项目从零编写的 Rust / Python 源代码 | **MIT** | ✅ 包含 |
| `data/open/heyu_open_sys.tsv` 等 (`data/open/*`) | **开源词表** | 由 [`tools/build_open_table.py`](../tools/build_open_table.py) 从 Unicode Unihan + `jieba/dict.txt` 独立生成 | **Unicode v3 + MIT** | ✅ 包含（公开版默认主码表） |
| `data/polyglot_{en,it,es}.tsv`, `data/zh_immersion_gloss.tsv`, `data/zh_context_ngrams.tsv`, `data/zh_turbo_phrases.tsv` | **原创词表** | 本项目自建四语词表与常用短语表 | **MIT** | ✅ 包含 |
| `data/examples/user_custom.example.tsv` | **原创示例** | 不含个人隐私的中性示例用户词表 | **MIT** | ✅ 包含（打包时复制为 `data/user_custom.tsv`） |
| `data/heyu_sys.tsv`, `data/heyu_reverse.tsv`, `data/shuangpin_shape_hints.tsv`, `data/root_atlas.json` | **受限词表 (仅限本地自用)** | 派生自 `amorphobia/openfly`（含小鹤音形 EULA / CC BY-NC 词表） | **受限 (禁止公开再分发)** | ❌ **绝不包含**（已从 Git 索引移除并列入硬编码黑名单） |
| `tools/heyu_facts.py`, `tools/gen_manual.py`, `tools/audit_manual.py`, `tools/raw_sources/*`, `鹤羽音形_完全指南.md` | **受限工具/文档 (仅限本地自用)** | 含小鹤字根表或基于受限词表生成的文档 | **受限 (禁止公开再分发)** | ❌ **绝不包含** |
| `data/user_custom.tsv`, `data/config.json`, `data/telemetry.json` | **个人本地数据** | 用户本机自定义词条（如「片语工作室」）、窗口坐标与打字统计 | **个人隐私数据** | ❌ **绝不包含** |

---

## 二、三级词表自动回退机制（本地打字零影响 + 公开版 100% 合规）

[`src/engine.rs`](../src/engine.rs) 在加载核心主码表、反查表与双拼形码提示表时，按以下三级优先级查找：

1. **第一优先级（用户私有目录）**：`%APPDATA%\HEYU\data\private\heyu_sys.tsv`
   - 您的原有词表已自动备份至该目录，永远不会被公开打包脚本触碰。
2. **第二优先级（工作区本地目录）**：`data\heyu_sys.tsv`
   - 保留在您的本地磁盘中供日常打字使用，但已在 `.gitignore` 中忽略并通过 `git rm --cached` 移出版本跟踪。
3. **第三优先级（公开开源默认词表）**：`data\open\heyu_open_sys.tsv`
   - 当上述两个私有文件不存在时（例如其他用户从 GitHub 克隆公开仓库或下载发布包），引擎自动加载基于 Unicode Unihan 与 `jieba` 构建的独立开源词表（覆盖 GB2312 6,762/6,763 字 = **99.99%**，含 **72,020** 条常用词组，共计 **104,548** 行）。

---

## 三、如何安全地公开发布到 GitHub（消除历史提交记录中的受限文件）

由于早期 Git 历史提交中曾包含 `heyu_sys.tsv` / `flypy_*.tsv`，直接推送当前 `master` 分支仍会在 `.git` 历史对象中暴露旧词表。
为此，项目提供了自动安全审计与无历史孤儿分支生成脚本 [`tools/make_release.ps1`](../tools/make_release.ps1)：

```powershell
# 生成干净的源码包 (dist/HEYU-1.0.0-src.zip)、运行包 (dist/HEYU-1.0.0-win64.zip)
# 以及彻底不含任何早期 Git 历史的全新孤儿分支 `public`：
powershell -ExecutionPolicy Bypass -File tools\make_release.ps1 -SkipBuild -PublicBranch -Force

# 将完全干净的 `public` 分支推送到 GitHub 公开仓库的 main 分支：
git push <您的GitHub远程地址> public:main --force
```

`tools/make_release.ps1` 具有五重硬性熔断检查（文件名黑名单、路径正则黑名单、本地受限词表 SHA-256 比对、敏感词扫描、大词表位置校验），任何受限或个人隐私文件一旦出现会立即阻断打包。
