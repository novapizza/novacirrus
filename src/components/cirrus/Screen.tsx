import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * Screen primitives — compose new pages with consistent rhythm.
 *
 *   <Screen>
 *     <ScreenHeader title="Transfers" subtitle="History & queue"
 *                   actions={<Button>New</Button>} />
 *     <ScreenBody>
 *       <Section title="Active">...</Section>
 *       <Section title="Completed">...</Section>
 *     </ScreenBody>
 *   </Screen>
 */

export function Screen({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <main className={cn("flex flex-1 flex-col overflow-hidden", className)}>{children}</main>
  );
}

export function ScreenHeader({
  title,
  subtitle,
  actions,
}: {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
}) {
  return (
    <header className="flex shrink-0 items-end justify-between gap-4 border-b border-border/60 px-6 py-4">
      <div className="min-w-0">
        <h1 className="font-display text-[17px] font-semibold leading-tight tracking-tight text-foreground">
          {title}
        </h1>
        {subtitle && (
          <p className="mt-0.5 truncate text-[12px] text-muted-foreground">{subtitle}</p>
        )}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  );
}

export function ScreenBody({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "scrollbar-mac flex-1 overflow-y-auto px-6 py-5 [&>*+*]:mt-6",
        className,
      )}
    >
      {children}
    </div>
  );
}

export function Section({
  title,
  description,
  children,
}: {
  title?: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <section>
      {(title || description) && (
        <div className="mb-2">
          {title && (
            <h2 className="text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
              {title}
            </h2>
          )}
          {description && (
            <p className="mt-0.5 text-[12px] text-muted-foreground/80">{description}</p>
          )}
        </div>
      )}
      <div className="rounded-xl border border-border/60 bg-card/60 p-1 popover-shadow">
        {children}
      </div>
    </section>
  );
}

export function Row({
  icon,
  label,
  hint,
  trailing,
}: {
  icon?: ReactNode;
  label: ReactNode;
  hint?: ReactNode;
  trailing?: ReactNode;
}) {
  return (
    <div className="flex items-center gap-3 rounded-lg px-3 py-2 hover:bg-accent/60">
      {icon && <div className="text-muted-foreground">{icon}</div>}
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] text-foreground">{label}</div>
        {hint && <div className="truncate text-[11px] text-muted-foreground">{hint}</div>}
      </div>
      {trailing && <div className="shrink-0 text-[12px] text-muted-foreground">{trailing}</div>}
    </div>
  );
}
