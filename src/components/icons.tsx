import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function Icon({ size = 15, children, ...props }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      {children}
    </svg>
  );
}

export const PlusIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M8 3v10M3 8h10" />
  </Icon>
);

export const PauseIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M5.5 3.5v9M10.5 3.5v9" />
  </Icon>
);

export const PlayIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M5 3.5 11.5 8 5 12.5v-9Z" fill="currentColor" stroke="none" />
  </Icon>
);

export const RetryIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M13 8a5 5 0 1 1-1.8-3.85M13 2.5V5h-2.5" />
  </Icon>
);

export const XIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="m4 4 8 8M12 4l-8 8" />
  </Icon>
);

export const TrashIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5 5 13h6l.5-8.5" />
  </Icon>
);

export const FolderIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M2 4.5A1 1 0 0 1 3 3.5h3.5L8 5h5a1 1 0 0 1 1 1v5.5a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1v-6Z" />
  </Icon>
);

export const OpenIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M11.5 2.5h2v2M13.5 2.5 8.5 7.5" />
    <path d="M6 4H3.5a1 1 0 0 0-1 1v7a1 1 0 0 0 1 1h7a1 1 0 0 0 1-1V9.5" />
  </Icon>
);

export const SearchIcon = (p: IconProps) => (
  <Icon {...p}>
    <circle cx="7" cy="7" r="4" />
    <path d="m10.5 10.5 3 3" />
  </Icon>
);

export const GearIcon = (p: IconProps) => (
  <Icon {...p}>
    <circle cx="8" cy="8" r="2" />
    <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.3 3.3l1.4 1.4M11.3 11.3l1.4 1.4M12.7 3.3l-1.4 1.4M4.7 11.3l-1.4 1.4" />
  </Icon>
);

export const ChevronUpIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="m4 10 4-4 4 4" />
  </Icon>
);

export const ChevronDownIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="m4 6 4 4 4-4" />
  </Icon>
);

export const SidebarIcon = (p: IconProps) => (
  <Icon {...p}>
    <rect x="2.5" y="3" width="11" height="10" rx="1.5" />
    <path d="M6.5 3v10" />
  </Icon>
);

export const GaugeIcon = (p: IconProps) => (
  <Icon {...p}>
    <path d="M2.5 11a5.5 5.5 0 0 1 11 0" />
    <path d="M8 11 10.5 7" strokeWidth="1.75" />
  </Icon>
);
