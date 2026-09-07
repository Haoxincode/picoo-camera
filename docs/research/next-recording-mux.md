# Next 录像封装与平台适配选型

关联 ARCH-PICOO-MEDIA-002、REQ-PICOO-NEXT-018/020/021/022。日期：2026-09-07。

## 需求与候选

原码流录像保存Receiver实际收到的完整AU，不解码、不重新编码；处理后录像的硬件编码结果也交给同一平台mux边界。两者使用有界队列、独立源时间线、配置分段和粘性的缺口/失败状态。平台mux不能替代这些Picoo语义，也不能阻塞Receiver owner。

按产品§17.3采用平台原生mux adapter。已核对候选：

| 候选 | 当前版本与维护证据 | 适用性 |
| --- | --- | --- |
| Apple AVAssetWriter | 官方Xcode26.6 SDK；AVAssetWriterInput(mediaType:outputSettings:sourceFormatHint:)、nil outputSettings直通压缩sample、readyForMoreMediaData、movieFragmentInterval、异步finishWriting | 采用。系统维护MP4与AVC/HEVC sample description，无额外媒体运行时；已有Swift硬件probe，生产Rust可复用objc2生成绑定 |
| Windows MF MPEG-4/fragmented MPEG-4 sink | 项目Windows SDK；产品§17.3已规定 | 采用方向不变，但仍须Windows原生验证HEVC sample description、分片及最终化，Apple证据不可替代 |
| mp4-rust | crates.io mp4 0.14.0，2023-08-01发布，仓库未归档，最近push为2024-06-15；MIT；crate约48KB，未声明MSRV | 有HEVC track与write_sample，但较久未维护，且不能直接替代平台原生媒体资源与已对齐的adapter契约；不引入 |
| scuffle-mp4 | 0.1.5，2025-06-16发布，单仓库未归档，最近push为2026-04-19；MIT/Apache-2.0，crate约47KB，未声明MSRV | 成熟MP4 box基础，仍需应用维护mux策略和平台交接；本项目已有Scuffle位流依赖不构成再接管容器实现的理由 |
| ffmpeg-next | 9.0.0，2026-08-05发布，WTFPL wrapper，crate约122KB，未声明MSRV | libavformat能力完整，但需要另行构建/分发FFmpeg原生运行时并核对其构建许可证、平台及体积；当前官方mux满足需求，不增加该发布负担 |

维护信息来自crates.io API及GitHub仓库元数据，不把仓库push时间等同于每个子包近期维护。Rust Apple绑定objc2-av-foundation 0.3.2已核对实际生成API，最低Rust1.71，edition2021，MIT/Apache-2.0/Zlib，可选必要feature，满足项目工具链；只链接Apple系统framework。具体增量须在生产feature集接入后测量，crate压缩包大小不是应用体积。官方AVAssetWriter基本API自macOS10.7可用，低于项目macOS最低版本；Windows adapter仅Windows原生构建。

参考：[Apple AVAssetWriter](https://developer.apple.com/documentation/avfoundation/avassetwriter)、[objc2](https://github.com/madsmtm/objc2)、[mp4-rust](https://github.com/alfg/mp4-rust)、[Scuffle](https://github.com/scufflecloud/scuffle)、[ffmpeg-next](https://github.com/zmwangx/rust-ffmpeg)。

## Apple合成原码流验证

[apple-mux-probe.swift](../../verification/native-media/apple-mux-probe.swift)使用现有iOS生产硬件编码器生成的合成AU/avcC/hvcC。CMFormatDescription用标准SampleDescriptionExtensionAtoms承载配置记录；AVAssetWriterInput的outputSettings为nil，显式sourceFormatHint。不通过像素缓冲或重新编码写入。相同IDR重复构成已知测试时间线，仅验证封装，不作为持续编码性能证据。

M4/macOS26.6.2实测AVC/HEVC×720p/1080p×30/60八组合，每个文件有91或181个AU、精确1/fps PTS。AVAssetReader逐AU字节与PTS回读一致。FFprobe独立解码计数、尺寸、帧率和BT.709 primaries/transfer/matrix全部正确。AVAssetReaderOutput官方头文件明确允许nil outputSettings时返回numSamples=0的marker；probe只把有sample的数据计为AU，仍检查完整时间线和最后reader状态。

movieFragmentInterval设为0.5秒仅用于故障探针，生产按产品要求约10秒分段并请求RAP。正常finish后的文件可被平台整理为非fragment结构，不能仅检查完成文件中moof数量判断写入中是否有恢复点。刻意在写完3秒零1帧后直接退出probe进程，跳过finalize：AVC与HEVC的720p30部分文件均可独立读取3秒/90帧，AVC检查保留moov与5个moof；尾部1帧未持久化，证明必须保留.partial并报告不完整，不能声称崩溃后全部帧可恢复。

这不是产品录像完成：Receiver入口接线、队列/超时、gap依赖链、分段事务、manifest耐久性、目录权限、UI状态、Windows后端及真实磁盘故障仍需实现和验证。probe不保存真实相机帧。

## Rust原生段适配验证

picoo-recording复用同一官方AVAssetWriter接口和objc2-av-foundation0.3.2，严格只启用Writer/Input/媒体类型及CoreMedia所需feature；系统Reader仅测试使用。Cargo新增AVFoundation绑定，锁文件也记录CoreMedia的可选CoreAudio绑定，但本次`cargo tree -p picoo-recording -e features -i objc2-core-audio`没有激活路径；没有FFmpeg分发库或新编解码器。应用体积增量仍待正式Receiver接线后测量，不以库文件大小推断。

原生Rust测试使用仓库已验证的AVC/HEVC720p合成fixture，30/60fps各五个AU；AVAssetReader通过现代异步loadTracks API回读，压缩字节及每个微秒PTS精确一致。原生Input必须显式设置mediaTimeScale=1000000。已有文件不覆盖，非IDR/非零段首、重复PTS和u64时间溢出均拒绝。最终化后的.partial文件在测试模拟owner改名为.mp4后可被系统读取，partial不作为已完成可播放结果。

原生最终化通知等待上限10秒；超时失败后的系统取消可能受原生I/O时长影响，只允许在专用录像工作者进行，不能据此宣称任意磁盘卡死下该线程一定在10秒内退出。录像owner的独立状态/超时与资源保留仍须在接线时实现。段适配无未完成像素/GPU工作，不把原生writer的内部资源寿命改成自制引用规则。

## Bundle文件操作复用

REQ-PICOO-MEDIA-067复用仓库已有serde/serde_json、sha2 0.10及tempfile 3生态，不自行实现随机命名、JSON或SHA-256。tempfile采用MIT/Apache-2.0，支持Windows/macOS及当前Rust工具链；使用独占目录、NamedTempFile原子替换和TempPath.persist_noclobber，不引入媒体runtime。摘要以64KiB缓冲流式计算，段/gap各限4096项。Unix目录0700、manifest0600；文件sync_all和Unix目录sync配合，Windows目录持久性与断电仍需原生环境验证，不能由macOS文件系统测试替代。所有操作属于专用录制工作者，不能在Receiver或UI事件循环调用。

## 完整AU入口队列

REQ-PICOO-MEDIA-068复用std::sync::mpsc::sync_channel及try_send，沿用FrameBus已有标准库通道实践；不新增crossbeam或自研无锁队列。单生产者Receiver、单消费者录制工作者没有多消费者需求。通道固定16项、每AU最大2MiB、配置最大64KiB，最坏待处理载荷约33MiB；250ms入口年龄上限由工作者检查。年龄超限和容量耗尽显式终止本次录像，正常关闭排空已接纳输入。排序/依赖链由录制工作者另外负责，通道只保留抵达次序。

## 原生取消的故障发现

EncodedWriter故障注入显示，AVAssetWriter cancelWriting可能删除尚未最终化输出；此前“finalize失败必定保留partial”不成立。正常输入被拒绝时，仍健康的writer先最终化有效前缀并登记段，整体结果再标Failed。原生writer失败后的partial留存仍需独立实现与故障验证。进程直接退出probe留下的fragment结果不能代替主动cancel路径。
