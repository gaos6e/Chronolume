# 官方参考价格

基础快照核对于 2026-10-26 至 2026-10-27，2026-11-07 补充混元、MiMo 及部分主流型号。2026-11-17 同步 OpenAI、Anthropic、Google、阿里云与腾讯混元的新模型。价格单位为 USD / 百万 Token。`src-tauri/src/pricing.rs` 保留 OpenAI 与 Anthropic 基础表，`src-tauri/src/pricing/official-prices.json` 保存其他品牌、补充档位、别名和逐型号来源。

| 品牌／渠道 | 官方来源 | 口径 |
| --- | --- | --- |
| OpenAI | [GPT-6.2 Astra](https://developers.openai.com/api/docs/models/gpt-6.2-astra)、[GPT-6.2 Sol](https://developers.openai.com/api/docs/models/gpt-6.2-sol)、[GPT-6 Cyber](https://developers.openai.com/api/docs/models/gpt-6-cyber)、[模型与价格](https://developers.openai.com/api/docs/pricing)、[GPT-6.1 Terra](https://developers.openai.com/api/docs/models/gpt-6.1-terra) | GPT-6.2 Astra、Sol 和 GPT-6 Cyber 收录标准价及输入超过 272,000 Token 的档位。GPT-6.2 的缓存写入按 30 分钟估算。GPT-6 Cyber 长上下文缓存写入单价待核验。另收录 GPT-6.2 Astra for Law 和 GPT-6 Rosalind 的官方参考价。`gpt-6.1-terra`：输入 $1、缓存读 $0.05、30 分钟缓存写入 $1.25、输出 $5；`gpt-6.1-sol`（含已统一的历史别名）：输入 $2、缓存读 $0.10、写 $2.50、输出 $10 |
| Anthropic | [Pricing](https://platform.claude.com/docs/en/about-claude/pricing) | 标准全球 API，含 Claude Fable 5.2 和 Mythos 5.2。Haiku 5.5 >100K 与 Sonnet 4.5 >200K 档位；5 分钟缓存写入 1.25 倍输入，1 小时为 2 倍 |
| DeepSeek | [Models & Pricing](https://api-docs.deepseek.com/quick_start/pricing/) | Pro 高峰输入 $1.32／缓存读 $0.044／输出 $3.96；Flash $0.30／$0.006／$1.20；非高峰减半 |
| Gemini | [Developer API pricing](https://ai.google.dev/gemini-api/docs/pricing) | 标准付费文本；Pro >200K 档位，收录 Gemini 4 Carbon、Hydrogen 和 Gemini Decision 1。已公布 Carbon 和 Argon 2026-12-01、3.6/3.7/3.8 Flash 2027-01-01 新价 |
| GLM | [Z.AI pricing](https://docs.z.ai/guides/overview/pricing) | 官网美元 Token 价，明确免费型号为零 |
| MiniMax | [Pricing overview](https://platform.minimax.io/docs/pricing/overview) | M3、M3.1 Flash 采用标准价，M3 >512K 使用长上下文档位。M2.7 系列采用 [MiniMax 官方按量价](https://platform.minimax.io/subscribe/token-plan?tab=api-enterprise) |
| Kimi | [Model pricing](https://platform.kimi.ai/docs/pricing/chat) | K3.1 Pro、Highspeed、K3、K2.7 Code、Highspeed、K2.6；K3 原生 1 小时缓存写入为输入价 2 倍 |
| Qwen | [Model Studio pricing](https://www.alibabacloud.com/help/en/model-studio/model-pricing)、[缓存](https://www.alibabacloud.com/help/en/model-studio/context-cache) | 新加坡 International 标准价，含 Qwen4-Max 的 5 分钟缓存写入价。Plus >256K 档位；3.8 Flash 缓存专属价。Qwen3.8 Max 等新增型号的缓存命中价待核验，含缓存命中的请求暂不定价 |
| Grok | [API pricing](https://docs.x.ai/developers/pricing) | 全球标准价；单次输入 ≥200K 为长上下文档位，不推断 US 区域附加费 |
| 腾讯混元 | [TokenHub 模型价格](https://www.tencentcloud.com/document/product/1300/78937) | Hy4、Hy4 Preview、Hy3 和 Hy4 Flash 采用腾讯云国际站美元价。Hy4 Flash 的五折优惠在北京时间 2026-11-30 12:00 结束 |
| 小米 MiMo | [API Pricing](https://mimo.mi.com/docs/price) | V2.6、V2.5 系列采用海外实时 API 价 |

DeepSeek 高峰为周一至周五 UTC 01:00–04:00、06:00–10:00；其余时段及中国公共假期为非高峰。假期依据[国务院 2026 年通知（政府门户转载）](https://www.beijing.gov.cn/cs/gncs/zcwj/202603/t20260327_4568275.html)。当前假日快照仅覆盖 2026 年，其他年份只应用周几和时段规则，更新版本应补充新年假日。

这是按已核验官网规则对日志用量进行的参考估算，使用当前价格快照；除已收录的未来生效日期外，不重建历年促销。未记录的订阅权益、税费、区域、服务等级、音频差价、付费搜索和缓存存储费用均未计入。同一模型在各平台使用供应商的官方 API 参考价，用户自定义价格也按模型全局生效。原始 provider 继续用于来源筛选，日志自报金额仍单独保存。

平台追加的 Fast、Free 标签在统计和计价前去除，如 `hy4free` 归为 `hy4`，`glm-5.3-flash-free` 归为 `glm-5.3-flash`。Flash、FlashX、Preview 等独立型号保留。已知型号缺少官方单价时显示未定价，模型信息缺失时显示未记录型号。

Schema 10 合并历史别名、移除旧内置渠道报价，并按新口径重算历史费用。旧渠道自定义价格归并为全局覆盖，同一模型有多项显式设置时保留最近更新的一项。仍可访问的非 Codex 来源重新读取原生用量，以应用请求级计价档位。只剩历史摘要时，保留已记录的档位，缺少档位时按标准价估算。

2026-11-21 新增 Gemini 4 Neon 的标准文本参考价。每百万 Token 的新增输入、缓存读取、输出分别为 3、0.15、15 美元。模型列表按基础型号汇总各思考档位。缓存命中率使用合并后的缓存读取总量除以对应输入总量。[Google AI Studio 定价](https://aistudio.google.com/docs/pricing#gemini-4-neon)

## 历史验收记录

本机验收（2026-10-26）：131 条价格行（包括档位及旧表条目）、9 个模型品牌及 OpenCode 渠道；基础型号历史别名的 6 次用量、DeepSeek Pro 70 次用量、OpenCode Flash Free 91 次用量与独立 Decimal 复算逐条一致。费用依次为 $0.030376、$1.041075、$0；原会话、用户设置及自定义价格保留。117 个 Rust 测试、45 个前端测试、clippy、lint、类型检查及安装/便携启动验证通过。

同日身份统一验收：Schema 7 合并 `gpt-6.1-sol` 的历史别名，正式库的模型、事件、片段、永久摘要、游标、延迟和价格列均无旧名称记录；原有明细的事件键、时间、Token 与成本不变。隔离迁移副本的数量、成本及计时合计一致，119 个 Rust 测试与 45 个前端测试通过，本机安装版和便携版启动正常。
