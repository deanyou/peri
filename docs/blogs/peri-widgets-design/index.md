# Peri Code 的终端组件库设计：怎么把 13 个组件做到零业务耦合

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。<https://github.com/KonghaYao/peri>

一个 Coding Agent 的对话界面需要同时渲染 Markdown 格式的模型回复、带行号和 hunk 头的代码 diff、有状态指示符的工具调用卡片、可交互的文件树和表单面板。如果每一样都直接在 TUI 应用代码里手写渲染逻辑，不出两个月代码就变成一座耦合的大厦——改一个颜色常量要碰十个文件，加一种消息类型需要新开十几个 match 分支。

Peri Code 的做法是把终端 UI 组件拆进一个独立的 crate——peri-widgets，49 个 .rs 文件，只依赖 ratatui（Rust 生态的终端 UI 框架）和 pulldown-cmark（CommonMark 标准的 Markdown 解析器）两个外部库。13 个模块分成四组——基础层垫底，渲染层、交互层、导航层依次往上——没有一行代码知道自己是在给 Coding Agent 工作。

**整体版图**。基础层提供 Theme（纯色接口，14 个方法）和 ScrollState（偏移管理和可见性判定）。渲染层包含 Markdown 事件驱动状态机、Diff 双层 LRU 缓存、MessageBlock 五态枚举调度、ToolCall 折叠策略。交互层是 Form trait 抽象、SelectableList 泛型闭包、ListOverlay 锚点自适应、RadioGroup/CheckboxGroup 解耦、InputField 字节级光标。导航层有 Spinner 随机动词池和 16 帧星号动画、FileTree 懒加载、TabBar 三态渲染。

## Theme 用 trait 接口切断颜色硬编码

Theme trait 定义了 14 个色值方法——accent、success、warning、error、thinking、text、muted、dim、border、border_active、popup_bg、cursor_bg、loading，外加几个 Diff 专用色。所有组件在需要颜色时调用 `theme.accent()` 或 `theme.error()`，而不是引用全局常量。

DarkTheme 是默认实现，色值从 peri-tui 的主题文件同步过来。组件不关心自己用的是 DarkTheme 还是其他主题——它只接受 `&dyn Theme`，调用方传什么就拿什么画。这意味着同一个 FileTree 在 peri-tui 里和 side-projects 的 git-graph 里可以渲染出完全不同的配色，而组件代码一行不变。

## ScrollState 一套滚动逻辑嵌入 ListState 和 FileTreeState

终端滚动有个容易踩的坑——鼠标滚轮事件是显示列偏移，但数据源按条目索引排列，中间要经过条目高度和列宽换算。ScrollState 封装了这层换算——它只管偏移量和 `ensure_visible`（确保指定索引在可视区域内），不关心数据源是什么。

ListState 和 FileTreeState 各自内嵌一个 ScrollState，复用同一套滚动逻辑。新增一个需要滚动的组件时——比如未来加一个树形面板——直接嵌一个 ScrollState 进去就行，不用重新实现滚动。

## Markdown 事件驱动状态机逐条消解 Event 流

pulldown-cmark 的解析输出是一串 Event——段落开始事件、文本事件、代码块开始事件、行内代码事件、段落结束事件，依次迭代。不需要先构建 AST（抽象语法树）再递归遍历。

RenderState 维护一个状态栈——styles_stack 追踪当前正在渲染的样式（粗体、斜体等），list_stack 追踪嵌套列表的层级和编号，quote_depth 追踪引用块的深度，in_code_block 标记是否在代码块内。每收到一个 Event，状态机更新对应栈、生成 ratatui 的 Span 或 Line，然后把控制权还给主循环。一次迭代跑完整篇文档，不分配中间树结构。

## Markdown 表格用 unicode-width 计算 CJK 视觉宽度

终端里 ASCII 字母占 1 列显示宽度，CJK（中日韩统一表意文字）占 2 列。如果按字符数等分列宽，中文内容会把表格撑歪——一个 4 个汉字的单元格视觉上占 8 列，但字符计数只到 4。

TableBuilder 在处理表格时先扫描全部行，用 unicode-width 的 `.width()` 方法计算每列的最大视觉宽度。列宽不够时，通过 `distribute_col_widths` 先保证每列的最小宽度（上限 10 列宽），剩余空间按理想宽度比例分配。单元格内长文本自动换行，换行后缩进到内容起始位置而非行首，读起来像段落的自然折行而非表格框架强行截断。

## Diff 计算和渲染各有一层 LRU 缓存

`compute_diff` 用 similar crate（Rust 的 diff 算法库）做行级 diff，结果缓存到全局 LruCache（最近最少使用淘汰策略，容量 64），key 是对 old/new 内容做的哈希。`render_diff` 把 DiffResult 渲染成 ratatui 的 Line 数组，又有一层专门的渲染缓存——同样是 64 容量。

Agent 反复展示同一份 diff 时——比如用户连续追问同一个文件的某处改动——两次计算和两次渲染全部命中缓存，零开销。缓存 key 用内容哈希而非原始字符串，避免在缓存里囤积大文本。超过 1MB 的 diff 输入直接截断，不做计算。

## Diff 词级变更超过四成不拆分着色

`compute_word_diff` 用 similar 的 `TextDiff::from_words` 计算行内的逐词 diff，能把一次重命名从「整行标红 + 整行标绿」变成「单词内几段红几段绿」。但如果一整行的大部分都变了——比如把一整行代码从 Rust 语法改成 Python 语法——词级高亮会产生大量红绿碎片，比单纯的单色 add/remove 更难读。

阈值设在 40%。用 word diff 先算一遍变更字符数，除以新旧文本总字符数，超过 40% 就跳过逐词着色，整行保留为单色 add 或 remove。低于阈值才拆分成词级高亮。用户看到的是干净的 diff——该细看的地方有词级颜色引导，大面积重写的地方单色提示就够了。

## MessageBlock 五类渲染场景用一个枚举派发

Agent 回复的消息不是只有文字。一条回复里可能夹杂文本段落、工具调用卡片、子代理的独立输出、思考过程摘要、系统通知。MessageBlockWidget 用一个 BlockRenderStrategy 枚举统一调度这五类场景——Text、ToolCall、SubAgent、Thinking、SystemNote——当前消息属于哪一类就 match 到对应的变体，渲染逻辑封装在变体内部。

新增消息类型时——比如某天要加一个「代码审查结果」块——加一个枚举变体，实现对应的渲染，调度框架不需要改动。两层结构（MessageBlockWidget → BlockRenderStrategy）把「消息块类型的分类」和「每种类型的渲染细节」拆开了。

## ToolCall 只读工具的调用结果默认折叠

工具调用的结果展示有一个信息量问题——模型读了一个文件、做了几次 grep 搜索，结果可能有几百上千行，但用户并不需要看完每一行，尤其在结果只是模型用来做后续推理的中间数据时。ToolCallState 维护一个只读工具白名单——Read、Glob、Grep、AskUserQuestion。这四种工具的结果默认折叠，用户按 Enter 展开，展开后最多显示 20 行。写类工具（Write、Edit、Bash）则不折叠——用户需要确认代码改了什么、命令执行了没有。

## Form 用 FormField trait 抽象表单字段

FormField trait 只定义了三个方法——`label()` 返回字段名、`next()` 跳到下一个字段、`prev()` 跳回上一个字段。FormState 负责字段间 Tab 切换和键盘事件委托，不关心字段的具体内容是什么。

调用方定义自己的枚举实现 FormField——比如「模型选择」表单里定义 ModelField 枚举，变体是 ProviderName、ModelName、ThinkingEffort——每个变体返回不同的 label，相邻关系由枚举变体顺序决定。组件层面不预设任何表单结构，有多少字段、字段间什么顺序，全是调用方的事。

## SelectableList 泛型加闭包消除数据类型依赖

`SelectableList<T>` 的渲染方法是 `Fn(&T, bool, bool) -> Line`——一个闭包，三个参数是条目引用、是否选中、是否悬停，返回一行文本。T 可以是文件路径、session ID、工具名称、或者是调用方自己定义的任何数据类型。

ListState 负责光标移动、滚动偏移、鼠标悬停检测，渲染完全交给闭包。组件不知道 T 是什么，也不需要知道——这种彻底的类型无关性靠泛型加闭包模板实现，不需要运行时多态。

## ListOverlay 根据可用空间自动选锚点方向

浮动面板最常见的 bug 是画出屏幕——面板在下方打开，但目标锚点离底部太近，面板内容被截断。ListOverlay 先测量锚点下方剩余行数，不够就换上方，上方也不够就居中显示。不依赖调用方手动计算位置，组件自己做空间检测和锚点切换。

## RadioGroup 选中和光标解耦

RadioGroup 维护两组独立状态——`selected` 是当前被选中的选项索引，`cursor` 是高亮移动的位置。用方向键上下移动光标时不改变选中状态，按 Enter 才确认选择。CheckboxGroup 同理——`checked` 集合和 `cursor` 索引分开维护，额外提供全选（toggle all）和全消（clear all）操作。

这种分离解决了一个常见的交互问题——用户浏览选项时不想触发选择，确认前需要看清楚每个选项的说明文字。光标和选中绑定在一起的组件做不到这一点——每按一次方向键就是一次选择。

## InputField 光标走 UTF-8 字节偏移

终端输入框的光标位置用字节偏移是唯一正确的方案。Rust 的 String 索引基于字节，char 是不定长编码（1-4 字节），字符计数和字节偏移会越走越岔。

InputField 的插入、删除、左右移动全部走字节偏移。masked 模式用于密码输入——前 4 个和后 4 个字符可见，中间用「•」遮罩，光标移动不受影响。

## Spinner 每次从动词池随机选短语

128 个动词短语分成九类——烹饪（烹制中、烘焙中、煎制中…）、思考（分析中、推敲中、琢磨中…）、构建（编写中、雕琢中、锻造中…）、搜索、运动、幻想（魔法中、炼金中、量子中…）、自然（发芽中、光合中、结霜中…）、幽默（捣鼓中、摸鱼中、挠头中…）、概念（重组中、编织中、凝结中…），从严肃到戏谑覆盖了完整的情绪带。每次状态变更调用 `pick_verb()` 从 128 个里随机挑一个，配合 16 帧星号动画（✳✴✵✶✷✸✹✺✻✼❃❊…的往复循环）形成流动的加载动效。`smooth_increment` 做渐进式 token 计数——显示的 token 数和实际 token 数有差距时，差距小（<70）每秒递增 3，中等（70-200）按 15% 比例加速，大的（>200）每次跳 50，形成一种「先快后慢追平」的节奏。调用方还可以通过 `active_form` 覆盖动词显示——比如 SubAgent 在执行具体任务时把「思考中」覆盖为「分析文件…」，用户一眼就知道 Agent 当前在做什么。

## FileTree 懒加载展开只加载当前层

FileTree 的数据模型是 FileNode——每个节点维护一个 `needs_load` 标记，展开时只加载当前目录的下一层子节点，子目录继续标记为 needs_load，不递归加载整棵树。FlatNode 把树拍平成线性列表供虚拟滚动渲染，排序时目录始终排在文件前面。

Toggle 操作只做两件事——调换展开/折叠状态、触发当前层的懒加载。1000 个文件的目录展开时只加载直接子项，孙节点等到用户真的翻到那里再加载。

## TabBar 三态各有独立渲染样式

TabBar 的三个状态对应三个颜色和分隔符——active 用 accent 色和实线分隔符、completed 用 success 色、incomplete 用 dim 色和点线分隔符。Tab 之间的分隔符独立于文本渲染，由 TabStyle 枚举描述。切换 tab 时只改 active 索引，TabBar 重绘时用三态渲染出「哪个是当前、哪些已完成、哪些还没走到」的视觉层次。

---

这 13 个组件没有 import peri-agent、peri-acp 或任何其他 workspace crate。它们唯一知道的领域是终端渲染本身——怎么画颜色、怎么算列宽、怎么管滚动。这种「不知道」不是功能缺失，是刻意抽离的结果。Peri Code 的主界面在用这些组件，side-projects 里的 git-graph 也在用它们——同一个 FileTree 渲染两棵完全不同的树、同一个 SelectableList 呈现 session 列表和插件列表、同一个 Theme 面对两套截然不同的配色方案——没有一行代码需要知道这些使用场景的存在。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
