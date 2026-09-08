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

## 录制短时重排

REQ-PICOO-MEDIA-070使用标准库BTreeMap持有最多16项完整AU，50ms从各项首次进入重排计时，不随轮询或后续包重置。已有picoo-jitter包含live播放期限和参考帧丢弃语义，不能复用于要求保留已收到完整AU的录制；保留的自研范围仅为AU顺序与世代排空这一Picoo边界。缺号后仍输出所有可用完整AU，由EncodedWriter报告依赖链缺口并等IDR。

## 跨通道早到的完整AU

REQ-PICOO-MEDIA-073采用标准库VecDeque建立独立于live单IDR缓存的配置等待门禁。最多16项，每项2MiB，配置64KiB，250ms期限从首次接收完整AU计时；后续配置与轮询不重置期限。每项绑定连接代际和stream epoch，已知配置以Arc保留；未知配置阻挡后续项直到解析身份匹配，不能借用当前旧配置。停止仍有未知配置或超限时终止录像，不把丢弃记为普通成功。此暂存预算独立于worker通道/重排，需纳入尚待完成的全局预算。

## 取消后的字节留存实现

REQ-PICOO-MEDIA-074在startWriting成功后以std::fs::File保留原生输出的只读句柄；本机验证此时路径已存在。AVAssetWriter取消删除路径后，句柄仍可读取原inode的已写入字节。取消完成后用标准io::copy和既有tempfile原子无覆盖提升恢复partial，并同步文件/目录；不要求文件系统支持硬链接，不复制正常录像路径。重复恢复核对设备号/inode，外来替换路径不覆盖且报错。显式finish/cancel返回恢复失败，Drop仅尽力清理。

macOS生产AVC/HEVC各91个合成AU取消测试：保留句柄与恢复文件逐字节相同；取消后不能签发完成凭据。该证据不承诺任意partial可播放，也不承诺存储故障或原生cancel阻塞有固定结束时限。磁盘满等物理写入失败仍返回失败。

## 桌面录像交互复用

REQ-PICOO-MEDIA-075复用当前锁定GPUI的prompt_for_paths目录选择、reveal_path系统定位及GPUI Kit Button/AlertDialog；沿用Receiver有界命令与异步reply，无新增UI依赖或自制选择器。操作区只显示简短结果，原生错误通过可键盘操作的详情按钮呈现，避免长路径/错误撑大工作区。取消选择不发送开始命令，队列拒绝明确回复；录制状态来自Core，Windows未接入原生mux时不显示可用入口。

## 原生工作者停滞观察

REQ-PICOO-MEDIA-076复用标准库Instant与AtomicU64记录工作者推进时间，由Receiver快照独立读取；无需增加timer线程、watchdog库或锁。15秒观察阈值大于原生finish回调的10秒等待上限；创建、原生写入、段提交和清理受系统I/O影响时仍可被外部观察。无响应仅是未推进事实，不强杀线程、不发完成凭据；系统调用恢复后的最终结果仍由owner决定。满队列/超龄按既有失败规则处理，已失败入口不再产生RAP需求。实际不可中断I/O的强制退出与进程级隔离不在此观察能力中被假称已解决。

## 跨阶段压缩输入预算

REQ-PICOO-MEDIA-077沿用仓库GPU completion permit的标准库原子计数与RAII释放模式，不增加异步运行时或通用队列依赖。进程唯一预算16MiB，AU首次进入录制所有权时预留payload长度与64KiB最大配置记录；即使配置尚未到达也已计入。计费凭据随ConfigurationWait→RecordingInput→通道→重排→写入移动，不可clone，清理或写入返回自动释放。为相同Arc配置多次保守预留可避免另造共享配置缓存/去重预算器。平台mux内部复制和网络重组分别受各自边界约束，不以这16MiB宣称全进程内存总量。

## 跨阶段提交期限

REQ-PICOO-MEDIA-078继续使用标准库Instant，在录制接纳完整AU时生成2秒绝对期限，随输入穿过配置等待、通道和重排，不读取远端时钟。各局部队列仍保留更短的容量/等待限制；停止排空也不豁免总期限。原生段切换可能等待最终化，因此每张AU在调用前、Busy重试前及实际写入返回后再次检查；已写入内容仍记入有效前缀，整体Failed及manifest保留具体原因。2秒限制是准入/返回检查，不冒充系统调用可中断或录像最终化整体硬上限。

## Windows原生mux探针

2026-09-08复核[Microsoft MPEG-4 File Sink](https://learn.microsoft.com/en-us/windows/win32/medfound/mpeg-4-file-sink)：AVC输入必须Annex B，MF_MT_MPEG_SEQUENCE_HEADER也使用SPS/PPS start codes；非自动支持的编码通过MF_MT_MPEG4_SAMPLE_DESCRIPTION提供完整stsd。使用既有windows 0.62.2生成绑定，仅启用MediaFoundation/COM，无额外媒体运行时。MFCreateMPEG4MediaSink自Windows 7、fragmented sink自Windows 8可用，低于项目Windows 11目标。

`picoo-recording/examples/windows_mux_probe.rs`以已有八组合合成AU分别测试普通/fragmented sink。sample description使用Apple原生参考box，不在探针阶段自研MP4生成器。AVC按官方要求转Annex B；HEVC测试hvc1配长度前缀AU，这是待Windows证实的候选契约，不是生产结论。SinkWriter输入输出使用相同压缩类型，不启用编码；SourceReader回读逐张VCL字节、帧数和PTS（容器时间舍入容许1微秒）。这些结果仍不替代完整参数/色彩、硬件Decoder、崩溃恢复或生产线程接线。

探针由cargo xtask test windows调用，CI保留只含合成影像的输出作为独立检查素材。本机仅编译非Windows入口，Windows分支必须等原生runner执行并按日志迭代。

首次Windows原生CI34172638264成功编译探针，但首个AVC普通sink在SetInputMediaType返回MF_E_INVALIDSTREAMNUMBER（0xC00D36B3）。原因边界是混用了IMFStreamSink.GetIdentifier与SinkWriter的零基stream index；探针改用Writer index=0，并打印sink ID作为证据。尚未推进到编码数据写入，不据此判断HEVC或MP4内容是否被支持。

第二轮CI34173708088已能写入首个AVC普通MP4，日志在finalize标记后返回E_INVALIDARG。下载产物有完整moov，ffprobe独立解码得到720p的91帧，因此不能直接归因于Finalize。探针改为成功Finalize后、sink Shutdown前关闭byte stream，分别报告两个调用的错误；实际原因和全组合仍等待Windows重跑，未豁免任何原生错误。

CI34174554259的完整矩阵确认AVC八项Finalize均成功，额外ByteStream.Close均报E_INVALIDARG；下载的八个普通/fragmented文件均可独立解码91/181帧。因此去掉多余Close，保留Finalize失败门禁并正常释放sink/stream。HEVC长度前缀候选不成立：720p普通文件无可解码帧，1080p普通写入/最终化失败，四项fragmented类型被拒。下一轮使用明确Annex B AU及VPS/SPS/PPS sequence header，仍保留完整stsd并分别验证两种sink；这仍是待验证候选，不是产品支持声明。

探针同时比较系统生成与显式提供stsd两种方式，形成32项独立证据；官方资料对HEVC自动生成的支持描述不足，当前SDK行为必须以原生结果确认。若系统能维护description，就不引入应用box builder；若失败仍保留原始HRESULT与产物，不把未支持研究分支静默视为成功。

## Apple处理后录像硬编边界

REQ-PICOO-MEDIA-079复用现有iOS VideoEncoderPipeline的官方VTCompressionSession配置契约：RequireHardwareAcceleratedVideoEncoder、prepare后的UsingHardwareAcceleratedVideoEncoder检查、明确AVC High/HEVC Main、零重排、关闭HEVC open GOP和BT.709 limited。Rust复用仓库已有objc2-video-toolbox 0.3.2（MIT/Zlib/Apache-2.0），仅扩展VTCompressionSession/VTCompressionProperties/VTSession/block2绑定；无额外媒体运行时，macOS 15目标高于逐帧output-handler API最低系统要求。已核对本地生成绑定与Apple API注释：session会保留image，output handler可能跨线程并在提交返回前或后执行。

输入采用picoo-gpu已有RenderedImage，复用其完成/不可变/池引用合同，不CPU mapping。编码只在录像工作者执行；回调以提交时所有权绑定图像与结果，单个输入未结束时不接纳下一张，超期仍保留平台持有资源直至完成/失效清理。现有Swift Sender事件缓冲会为直播恢复丢弃预测链，不能直接复用于录像；仅复用平台配置规则，不把直播丢帧策略带入Recorder。

## Windows已验证的容器选择

CI34175223149的32项矩阵得到24项成功：AVC四格式×普通/fragmented×自动/提供description全通过；HEVC四格式×普通×两种description全通过。SourceReader逐AU VCL/帧数/PTS验证成功；下载的24个文件再经ffprobe独立解码，尺寸、91/181帧与BT.709 limited全部正确。新证据确认此Windows SDK可从Annex B参数集序列头自动生成合法HEVC description，不必应用维护stsd builder；旧文档未保证这一能力，所以支持仍以平台准入与文件证据为边界。

八项fragmented HEVC在创建sink时统一返回MF_E_INVALIDMEDIATYPE。产品§17.4限定在支持的后端使用fragmented MP4，故选择AVC fragmented、HEVC普通MP4，各自维持约10秒独立RAP分段；不尝试软件编码或运行中切换容器。探针仅把HEVC fragmented创建阶段这一确切HRESULT报告为UNSUPPORTED；所有普通HEVC失败、AVC失败、成功创建后的写入/最终化/回读失败仍使测试失败。此前32项全部视为必过属于研究矩阵假设，不能把该平台不支持的可选机制误算为普通HEVC不支持。原生Windows Recorder接线与崩溃恢复仍未完成。
