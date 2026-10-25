import type { ReactNode, RefObject } from "react";

import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface ModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  className?: string;
  initialFocus?: boolean | RefObject<HTMLElement | null>;
}

/** App-level dialog: title, optional lede, scrolling body, anchored footer. */
export function Modal({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  className = "sm:max-w-[33rem]",
  initialFocus,
}: ModalProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className={className} initialFocus={initialFocus}>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description ? <DialogDescription>{description}</DialogDescription> : null}
        </DialogHeader>
        <DialogBody>{children}</DialogBody>
        {footer ? <DialogFooter>{footer}</DialogFooter> : null}
      </DialogContent>
    </Dialog>
  );
}
