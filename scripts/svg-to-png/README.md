# SVG 图标转换规范

GUI 统一使用预转换的 PNG，不在运行时解析 SVG。

## 资源规则

- 标准工具图标输出为 64×64，背景透明。
- 每个图标同时提供 `-light.png` 和 `-dark.png`，保存在 `gui/assets/`。
- 浅色主题使用 `#20242B`，深色主题使用 `#F3F4F6`。
- GUI 通过 `include_bytes!` 嵌入 PNG，并为每个资源使用唯一的 `bytes://` URI。
- 源 SVG、两个主题的 PNG、转换脚本和锁文件一并提交；`node_modules` 不得提交。
- 生成后检查尺寸、透明背景、主题颜色和界面实际显示效果。

## 使用方法

在仓库根目录安装固定版本的依赖：

```bash
npm ci --prefix scripts/svg-to-png
```

通用转换命令：

```bash
node scripts/svg-to-png/convert.mjs <input.svg> <output.png> [--size 64] [--color #20242B]
```

`--size` 是输出宽度，范围为 1–4096；高度按 SVG 宽高比计算。`--color` 设置 SVG 根元素的继承填充色，适用于单色图标。

以 `rotate-right` 为例：

```bash
node scripts/svg-to-png/convert.mjs reference/rotate-right.svg gui/assets/rotate-right-light.png --size 64 --color '#20242B'
node scripts/svg-to-png/convert.mjs reference/rotate-right.svg gui/assets/rotate-right-dark.png --size 64 --color '#F3F4F6'
```
