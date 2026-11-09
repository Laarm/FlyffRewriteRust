// Button of the admin design system. Variants: primary (the one main action), secondary (default), ghost (toolbar),
// danger (destructive). `loading` disables the button and shows a spinner while keeping its width; an icon-only button
// needs `aria-label`.
import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react';
import { Icon, type IconName } from './Icon';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';

export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  readonly variant?: ButtonVariant;
  readonly size?: 'sm' | 'md';
  readonly icon?: IconName;
  readonly loading?: boolean;
  readonly children?: ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  {
    variant = 'secondary',
    size = 'md',
    icon,
    loading = false,
    disabled,
    className,
    children,
    type,
    ...rest
  },
  ref,
) {
  const classes = [
    'adm-btn',
    `adm-btn--${variant}`,
    size === 'sm' ? 'adm-btn--sm' : '',
    loading ? 'is-loading' : '',
    className ?? '',
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <button
      ref={ref}
      type={type ?? 'button'}
      className={classes}
      disabled={disabled === true || loading}
      aria-busy={loading || undefined}
      {...rest}
    >
      {loading ? (
        <span className="adm-spinner" aria-hidden="true" />
      ) : icon ? (
        <Icon name={icon} />
      ) : null}
      {children !== undefined && children !== null && (
        <span className="adm-btn__label">{children}</span>
      )}
    </button>
  );
});
