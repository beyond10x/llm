import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';
import {themes as prismThemes} from 'prism-react-renderer';
import docsSystemPlugin, {ecosystemFooterGroup, ecosystemNavbarItems} from '@beyond10x/docs-system/docusaurus';

const config: Config = {
  title: 'LLM',
  tagline:
    'One neutral model-inference boundary: injected credentials, bounded transport, explained routing, and accounting that never turns an unknown into a zero.',
  favicon: 'img/mark.svg',

  future: {v4: true},
  url: 'https://beyond10x.github.io',
  baseUrl: '/llm/',
  organizationName: 'beyond10x',
  projectName: 'llm',
  deploymentBranch: 'gh-pages',
  trailingSlash: false,
  onBrokenLinks: 'throw',

  markdown: {
    hooks: {onBrokenMarkdownLinks: 'throw'},
    mermaid: true,
  },
  themes: ['@docusaurus/theme-mermaid'],
  plugins: [docsSystemPlugin],
  i18n: {defaultLocale: 'en', locales: ['en']},

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          routeBasePath: 'docs',
          editUrl: 'https://github.com/beyond10x/llm/tree/main/website/',
        },
        blog: false,
        theme: {customCss: './src/css/custom.css'},
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    colorMode: {respectPrefersColorScheme: true},
    navbar: {
      title: 'LLM',
      logo: {alt: 'LLM', src: 'img/mark.svg', width: 26, height: 26},
      items: [
        ...ecosystemNavbarItems(),
        {type: 'docSidebar', sidebarId: 'docsSidebar', position: 'left', label: 'Documentation'},
        {to: '/docs/status/where-this-stands', label: 'What works today', position: 'left'},
        {
          href: 'https://github.com/beyond10x/llm',
          label: 'GitHub',
          position: 'right',
          className: 'navbar-github-link',
          'aria-label': 'GitHub repository',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        ecosystemFooterGroup(),
        {
          title: 'Documentation',
          items: [
            {label: 'Introduction', to: '/docs'},
            {label: 'Getting started', to: '/docs/getting-started'},
            {label: 'The neutral boundary', to: '/docs/concepts/overview'},
            {label: 'Crate reference', to: '/docs/reference/crates'},
          ],
        },
        {
          title: 'Build and verify',
          items: [
            {label: 'Run a local turn', to: '/docs/guides/run-a-local-turn'},
            {label: 'Explain a route', to: '/docs/guides/explain-a-route'},
            {label: 'Price recorded usage', to: '/docs/guides/price-recorded-usage'},
            {label: 'Run the checks', to: '/docs/guides/run-the-checks'},
          ],
        },
        {
          title: 'Project',
          items: [
            {label: 'Status', to: '/docs/status/where-this-stands'},
            {label: 'Limitations', to: '/docs/status/limitations'},
            {label: 'Roadmap', to: '/docs/status/roadmap'},
            {label: 'Source', href: 'https://github.com/beyond10x/llm'},
          ],
        },
      ],
      logo: {alt: 'LLM', src: 'img/mark.svg', href: '/', width: 22, height: 22},
      copyright:
        '<span class="footer__claim">Unknown is never zero.</span>' +
        'LLM · LicenseRef-B10x-Proprietary · built with Docusaurus.',
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['rust', 'toml', 'yaml', 'json', 'bash'],
    },
    mermaid: {theme: {light: 'neutral', dark: 'dark'}},
  } satisfies Preset.ThemeConfig,
};

export default config;
