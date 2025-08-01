import { defineConfig } from 'vitepress'

// https://vitepress.dev/reference/site-config
export default defineConfig({
  title: "solite",
  description: "A SQLite runtime, CLI, and Jupyter kernel",
  themeConfig: {
    // https://vitepress.dev/reference/default-theme-config
    nav: [
      { text: 'Home', link: '/' },
    ],

    sidebar: [
      {
        text: 'Getting Started',
        items: [
          { text: 'Installation', link: '/installing' },
        ]
      },
      {
        text: 'Guides',
        items: [
          { text: 'REPL', link: '/repl' },
          { text: 'SQL Scripts', link: '/sql-scripts' },
          { text: 'Jupyter Kernel', link: '/jupyter' },
          { text: 'Command Line SQL', link: '/command-line' },
          { text: 'Standard Library', link: '/stdlib' },
          { text: 'SQLite Extensions', link: '/sqlite-extensions' },
        ]
      },
      {
        text: 'Reference',
        items: [
          { text: 'CLI Reference', link: '/reference/cli' },
          { text: 'SQL Standard Library', link: '/reference/sql' },
          { text: 'Dot Commands', link: '/reference/dot' },
        ]
      },
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/vuejs/vitepress' }
    ]
  }
})
