# 感冒院视觉基线

系统一句话：以 `tech-utility` 为基础，做一套适合个人开发者长期使用的桌面工作台界面，强调安静、紧凑、可信和可操作。

## OKLch Tokens

```css
:root {
  --bg:      oklch(98% 0.005 250);
  --surface: oklch(100% 0 0);
  --fg:      oklch(22% 0.02 240);
  --muted:   oklch(50% 0.018 240);
  --border:  oklch(90% 0.008 240);
  --accent:  oklch(58% 0.16 145);
}
```

## 字体

- Display: `-apple-system, BlinkMacSystemFont, "Inter", "Segoe UI", system-ui, sans-serif`
- Body: `-apple-system, BlinkMacSystemFont, "Inter", "Segoe UI", system-ui, sans-serif`
- Mono: `"JetBrains Mono", "IBM Plex Mono", ui-monospace, Menlo, monospace`

## 姿态规则

1. 信息密度高，但只在列表、筛选、详情里密，不用大面积指标墙。
2. 主体是中性灰阶，强调色只用于当前操作、选中和积极状态。
3. 列表、抽屉、命令面板是核心交互容器，避免营销化大 Hero 和装饰图。
4. 状态标签使用轻着色底，不用高饱和色块覆盖整个界面。
5. 深浅主题都保留同一结构与层级，只切换表面亮度和边框对比。
