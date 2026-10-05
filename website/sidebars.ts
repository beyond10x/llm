import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docsSidebar: [
    {
      type: 'category',
      label: 'Start here',
      collapsed: false,
      items: ['index', 'getting-started', 'status'],
    },
    {
      type: 'category',
      label: 'Concepts',
      collapsed: false,
      items: [
        'concepts/overview',
        'concepts/neutral-boundary',
        'concepts/protocols',
        'concepts/credentials',
        'concepts/routing',
        'concepts/accounting',
      ],
    },
    {
      type: 'category',
      label: 'Guides',
      collapsed: false,
      items: [
        'guides/run-a-local-turn',
        'guides/call-a-local-endpoint',
        'guides/call-a-model-with-one-forced-tool',
        'guides/use-llm-from-a-synchronous-loop',
        'guides/explain-a-route',
        'guides/price-recorded-usage',
        'guides/resolve-a-local-secret',
        'guides/run-the-checks',
      ],
    },
    {
      type: 'category',
      label: 'Reference',
      collapsed: false,
      items: ['reference/crates', 'reference/formats'],
    },
    {
      type: 'category',
      label: 'Project status',
      collapsed: false,
      items: ['status/limitations', 'status/roadmap'],
    },
  ],
};

export default sidebars;
