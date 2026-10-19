import { defineConfig } from 'vitepress'

export default defineConfig({
  // GitHub Pages 项目站点的仓库名为 pinapp,站点挂载在 /pinapp/ 下。
  base: '/pinapp/',
  lang: 'zh-CN',
  title: 'pinapp',
  description:
    'Snipaste 风格的贴图工具,面向 danklinux(niri / Wayland),基于 gpui-kit。Print 截图,Pause 钉图。',
  appearance: 'force-dark',
  lastUpdated: true,
  themeConfig: {
    nav: [
      { text: '快速开始', link: '/guide/getting-started' },
      { text: '安装', link: '/guide/install' },
      { text: '常见问题', link: '/guide/faq' },
      { text: 'GitHub', link: 'https://github.com/Thy3634/pinapp', target: '_blank' },
    ],
    sidebar: [
      {
        text: '指南',
        items: [
          { text: '快速开始', link: '/guide/getting-started' },
          { text: '安装', link: '/guide/install' },
          { text: '常见问题', link: '/guide/faq' },
        ],
      },
    ],
    socialLinks: [
      { icon: 'github', link: 'https://github.com/Thy3634/pinapp' },
    ],
    footer: {
      message: 'pinapp · MIT License · 基于 gpui-kit 构建',
      copyright: 'Copyright © 2026 Thy3634',
    },
    search: {
      provider: 'local',
    },
  },
})
