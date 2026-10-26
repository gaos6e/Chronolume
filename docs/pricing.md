# 官方参考价格

核对日期：2026-10-26。价格单位为 USD / 百万 Token。`src-tauri/src/pricing.rs` 保留 OpenAI 与 Anthropic 基础表，`src-tauri/src/pricing/official-prices.json` 保存其他品牌、补充档位、别名和逐型号来源。

| 品牌／渠道 | 官方来源 | 口径 |
| --- | --- | --- |
| OpenAI | [模型与价格](https://developers.openai.com/api/docs/pricing) | 既有标准短上下文价；`gpt-6.1-sol`（含已统一的历史别名）：输入 $2、缓存读 $0.10、写 $2.50、输出 $10 |
| Anthropic | [Pricing](https://platform.claude.com/docs/en/about-claude/pricing) | 标准全球 API；Haiku 5.5 >100K 与 Sonnet 4.5 >200K 档位；5 分钟缓存写入 1.25 倍输入，1 小时为 2 倍 |
| DeepSeek | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/) | Pro 高峰输入 $1.32／缓存读 $0.044／输出 $3.96；Flash $0.30／$0.006／$1.20；非高峰减半 |
| Gemini | [Developer API pricing](https://ai.google.dev/gemini-api/docs/pricing) | 标准付费文本；Pro >200K 档位，已公布 Argon 2026-12-01、3.6/3.8 Flash 2027-01-01 新价 |
| GLM | [Z.AI pricing](https://docs.z.ai/guides/overview/pricing) | 官网美元 Token 价，明确免费型号为零 |
| MiniMax | [Pricing overview](https://platform.minimax.io/docs/pricing/overview) | M3、M3.1 Flash 标准价；M3 >512K 档位。旧 M2.x 当前仅收录已核对的 OpenCode 渠道价 |
| Kimi | [Model pricing](https://platform.kimi.ai/docs/pricing/chat) | K3.1 Pro、Highspeed、K3、K2.7 Code、Highspeed、K2.6；K3 原生 1 小时缓存写入为输入价 2 倍 |
| Qwen | [Model Studio pricing](https://www.alibabacloud.com/help/en/model-studio/model-pricing)、[缓存](https://www.alibabacloud.com/help/en/model-studio/context-cache) | 新加坡 International 标准价；Plus >256K 档位；3.8 Flash 缓存专属价。Qwen3.8 Max 直连区分显式与隐式缓存，现有日志不能可靠识别，暂不套用单一缓存价；OpenCode 已发布的该型号渠道价单独支持 |
| Grok | [API pricing](https://docs.x.ai/developers/pricing) | 全球标准价；单次输入 ≥200K 为长上下文档位，不推断 US 区域附加费 |
| OpenCode | [Console models](https://opencode.ai/v2/docs/console/models/) | 显式收录核验过的渠道价格和免费型号；不把任意 `-free` 后缀推广为其他渠道零价 |

DeepSeek 高峰为周一至周五 UTC 01:00–04:00、06:00–10:00；其余时段及中国公共假期为非高峰。假期依据[国务院 2026 年通知（政府门户转载）](https://www.beijing.gov.cn/cs/gncs/zcwj/202603/t20260327_4568275.html)。当前假日快照仅覆盖 2026 年，其他年份只应用周几和时段规则，更新版本应补充新年假日。

这是按已核验官网规则对日志用量进行的参考估算，使用当前价格快照；除已收录的未来生效日期外，不重建历年促销。未记录的订阅权益、税费、区域、服务等级、音频差价、付费搜索和缓存存储费用均未计入。原始 provider 保留，已确认的模型别名统一身份；用户显式覆盖优先。

匹配只接受已有官方精确 ID、已知日期/推理后缀及显式别名，未知家族或任意 Fast/Free 后缀不猜价。请求选定的档位随永久摘要保留；重新计价不以多次请求的汇总输入重新判断长上下文。

本机验收（2026-10-26）：131 条价格行（包括档位及旧表条目）、9 个模型品牌及 OpenCode 渠道；基础型号历史别名的 6 次用量、DeepSeek Pro 70 次用量、OpenCode Flash Free 91 次用量与独立 Decimal 复算逐条一致。费用依次为 $0.030376、$1.041075、$0；原会话、用户设置及自定义价格保留。117 个 Rust 测试、45 个前端测试、clippy、lint、类型检查及安装/便携启动验证通过。

同日身份统一验收：Schema 7 合并 `gpt-6.1-sol` 的历史别名，正式库的模型、事件、片段、永久摘要、游标、延迟和价格列均无旧名称记录；原有明细的事件键、时间、Token 与成本不变。隔离迁移副本的数量、成本及计时合计一致，119 个 Rust 测试与 45 个前端测试通过，本机安装版和便携版启动正常。
