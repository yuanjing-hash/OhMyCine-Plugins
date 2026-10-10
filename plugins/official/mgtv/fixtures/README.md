# 官方协议取证

取证日期：2026-10-10。以下均为匿名公开控制接口响应，不包含媒体字节或账号凭据。

| 文件 | 官方接口 |
| --- | --- |
| auth-generate.json | `https://nuc.api.mgtv.com/GetMultiPic`，真实匿名二维码生成响应，未扫码；rcode 和全部 QR 查询值脱敏，保留完整 JSON 结构与官方登录域名/path |
| channels.json | `https://pianku.api.mgtv.com/rider/config/platformChannels/v1?platform=pcweb` |
| channel.json | `https://pianku.api.mgtv.com/rider/config/channel/v1?channelId=2&platform=pcweb` |
| catalog.json | `https://pianku.api.mgtv.com/rider/list/pcweb/v3?platform=pcweb&channelId=2&pn=1&pc=2&hudong=1&_support=10000000` |
| info.json | `https://pcweb.api.mgtv.com/video/info?cid=778406&vid=24623021` |
| episodes.json | `https://pcweb.api.mgtv.com/episode/list?_support=10000000&version=5.5.35&video_id=24623021&page=0&size=30&platform=4&src=mgtv` |
| info-free.json | `https://pcweb.api.mgtv.com/video/info?cid=687100&vid=22025165` |
| stream-preview.json | `video/streamList`，`cid=778406&video_id=24623021&type=pch5&playType=1&definitionType=2&auth_mode=1&_support=10000000&allowedRC=1` |
| stream-free.json | 同一真实流接口，`cid=687100&video_id=22025165` |
| free-vod.m3u8 | 同一完整免费视频实际调度后的 HLS 控制清单，pcvideomigu.titan.mgtv.com；758 URI、ENDLIST，无媒体分片字节 |

两个流清单替换所有临时 `pm2/gsid`、`penc/sourceTicket` 和用户网络/账号字段为 `REDACTED`。这是对真实响应的脱敏，不是伪造已登录/VIP 成功响应。

HLS 控制清单替换全部 URL 查询参数值和 EXT-MGTV-X-PARM 为 REDACTED；保持真实标签、时长、序列和文件路径。原控制清单约 1.01 MiB，脱敏后约 270 KiB。

`src/auth.rs` 合成账户字段的单测专门检查 DTO 不泄露 ticket/手机号。扫码确认、Set-Cookie、会员已登录响应及成功搜索模板没有真实账号验收，不能将这些单测当成提供方成功登录证据。
