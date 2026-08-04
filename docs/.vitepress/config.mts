import { defineConfig } from 'vitepress'

// https://vitepress.dev/reference/site-config
export default defineConfig({
  title: "Porter",
  description: "3D Tiles streaming service",
  base: '/porter/',
  ignoreDeadLinks: true,
  themeConfig: {
    // https://vitepress.dev/reference/default-theme-config
    nav: [
      { text: 'Home', link: '/' },
      { text: 'Guide', link: '/guide/what-is-porter' },
      { text: 'API reference', link: '/api/layers' },
    ],

    sidebar: [
      {
        text: 'Guide',
        items: [
          { text: 'What is Porter?', link: '/guide/what-is-porter' },
          { text: 'Usage', link: '/guide/usage' },
          { text: 'Layer definitions', link: '/guide/layer-definitions' },
          { text: 'How it works', link: '/guide/how-it-works' },
          { text: 'Metrics', link: '/guide/metrics' },
        ],
      },
      {
        text: 'API reference',
        items: [
          { text: 'Discover layers', link: '/api/layers' },
          { text: '3D Tiles', link: '/api/3d-tiles' },
          { text: 'WMTS Simple imagery', link: '/api/wmts-simple-imagery' },
          { text: 'Mapzen Terrarium elevation', link: '/api/mapzen-terrarium-elevation' },
          { text: 'OGC API - Features', link: '/api/ogc-api-features' },
        ]
      }
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/mappend-io/porter' },
    ],
  },
})
