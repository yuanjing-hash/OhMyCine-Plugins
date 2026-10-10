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
| info-variety.json | `https://pcweb.api.mgtv.com/video/info?cid=887305`，密室大逃脱第八季，保留官方默认视频 ID |
| episodes-variety-page1.json、episodes-variety-page2.json | `https://pcweb.api.mgtv.com/episode/list?video_id=24691407&page=1&size=30&platform=4&src=mgtv&_support=10000000`，第 2 页只改变 `page=2` |
| dispatch-observations.json | 同一匿名完整免费流的三路官方调度，仅保留 dispatcher/hostname/status/scheme/port/format 六个标量；不保存媒体 URL 的 path/query 或响应原文 |
| info-free.json | `https://pcweb.api.mgtv.com/video/info?cid=687100&vid=22025165` |
| stream-preview.json | `video/streamList`，`cid=778406&video_id=24623021&type=pch5&playType=1&definitionType=2&auth_mode=1&_support=10000000&allowedRC=1` |
| stream-free.json | 同一真实流接口，`cid=687100&video_id=22025165` |
| free-vod.m3u8 | 同一完整免费视频实际调度后的 HLS 控制清单，pcvideomigu.titan.mgtv.com；758 URI、ENDLIST，无媒体分片字节 |

两个流清单替换所有临时 `pm2/gsid`、`penc/sourceTicket` 和用户网络/账号字段为 `REDACTED`。这是对真实响应的脱敏，不是伪造已登录/VIP 成功响应。

综艺响应的 `t1` 是目录排序号，`isIntact=1` 同时出现在正片、Plus 版和彩蛋上；不能直接当作真实集数。第 0 页会定位默认视频所在页，明确指定第 1 页才能从头读取。`short` 在各页重复，插件只在完整节目列表之后添加一次。详情/分集 fixture 删除 `seqid`、`seqId`、`timestamp` 和 `intranetUrl`；只保留公开目录信息。调度回归测试的完整 URL 和 path 显式使用 `SYNTHETIC`，仅域名来自匿名控制请求。

HLS 控制清单替换全部 URL 查询参数值和 EXT-MGTV-X-PARM 为 REDACTED；保持真实标签、时长、序列和文件路径。原控制清单约 1.01 MiB，脱敏后约 270 KiB。

`src/auth.rs` 合成账户字段的单测专门检查 DTO 不泄露 ticket/手机号。扫码确认、Set-Cookie、会员已登录响应及成功搜索模板没有真实账号验收，不能将这些单测当成提供方成功登录证据。
