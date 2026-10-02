import type { SVGProps } from 'react';

const base = {
  width: 22,
  height: 22,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.8,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

type P = SVGProps<SVGSVGElement>;

export const HomeIcon = (p: P) => (
  <svg {...base} {...p}>
    <path d="M3 11.5 12 4l9 7.5" />
    <path d="M5 10v9a1 1 0 0 0 1 1h4v-5h4v5h4a1 1 0 0 0 1-1v-9" />
  </svg>
);

export const SparklesIcon = (p: P) => (
  <svg {...base} {...p}>
    <path d="M12 3l1.8 4.7L18.5 9.5l-4.7 1.8L12 16l-1.8-4.7L5.5 9.5l4.7-1.8z" />
    <path d="M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z" />
  </svg>
);

export const UserIcon = (p: P) => (
  <svg {...base} {...p}>
    <circle cx="12" cy="8" r="4" />
    <path d="M4 20c0-3.5 3.6-6 8-6s8 2.5 8 6" />
  </svg>
);

export const PlusIcon = (p: P) => (
  <svg {...base} {...p}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);

export const PowerIcon = (p: P) => (
  <svg {...base} {...p}>
    <path d="M12 3v9" />
    <path d="M6.3 6.8a8 8 0 1 0 11.4 0" />
  </svg>
);

export const TrashIcon = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1l1-12M9 7V4h6v3" />
  </svg>
);

export const ShieldIcon = (p: P) => (
  <svg {...base} {...p}>
    <path d="M12 3l7 3v5c0 4.5-3 8-7 10-4-2-7-5.5-7-10V6z" />
    <path d="M9 12l2 2 4-4" />
  </svg>
);

export const ChatIcon = (p: P) => (
  <svg {...base} {...p}>
    <path d="M4 5h16a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H9l-5 4v-4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z" />
  </svg>
);

export const ChevronDownIcon = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M6 9l6 6 6-6" />
  </svg>
);

export const RefreshIcon = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M20 11a8 8 0 0 0-14.5-4M4 13a8 8 0 0 0 14.5 4" />
    <path d="M20 4v5h-5M4 20v-5h5" />
  </svg>
);

export const CheckIcon = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M5 12l5 5 9-10" />
  </svg>
);

export const BoltIcon = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M13 3L5 14h6l-1 7 8-11h-6z" />
  </svg>
);
