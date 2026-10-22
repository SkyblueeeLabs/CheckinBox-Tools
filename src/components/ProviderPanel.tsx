import type { AccountView, ProviderView } from "../lib/types";
import AccountCard from "./AccountCard";

interface Props {
  provider: ProviderView;
  accounts: AccountView[];
  busy: boolean;
  authPending: boolean;
  onCheckin: (providerId: string, accountId: string, label: string) => void;
  onDiscover: () => void;
  onAddAccount: () => void;
  onRemove: (providerId: string, accountId: string, label: string) => void;
}

export default function ProviderPanel({
  provider,
  accounts,
  busy,
  authPending,
  onCheckin,
  onDiscover,
  onAddAccount,
  onRemove,
}: Props) {
  return (
    <section className="provider" style={{ ["--accent" as string]: provider.accent }}>
      <header className="provider-head">
        <div className="provider-title">
          <span className="accent-bar" />
          <h2>{provider.name}</h2>
          <a
            className="docs"
            href={provider.docsUrl}
            onClick={(e) => {
              e.preventDefault();
              window.open(provider.docsUrl, "_blank");
            }}
          >
            活动说明 ↗
          </a>
        </div>
        <div className="provider-actions">
          {provider.supportsDiscovery && (
            <button className="ghost" onClick={onDiscover} disabled={busy}>
              从本机客户端导入
            </button>
          )}
          {provider.id === "qoder" && (
            <button className="ghost" onClick={onAddAccount} disabled={busy || authPending}>
              {authPending ? "等待授权…" : "添加账号"}
            </button>
          )}
        </div>
      </header>

      {accounts.length === 0 ? (
        <p className="empty">
          还没有账号。
          {provider.supportsDiscovery
            ? "点「从本机客户端导入」即可读取本机登录态。"
            : "点「添加账号」走一次浏览器授权。"}
        </p>
      ) : (
        <div className="accounts">
          {accounts.map((view) => (
            <AccountCard
              key={`${view.account.provider}:${view.account.id}`}
              view={view}
              busy={busy}
              onCheckin={() =>
                onCheckin(
                  view.account.provider,
                  view.account.id,
                  view.account.label
                )
              }
              onRemove={() =>
                onRemove(
                  view.account.provider,
                  view.account.id,
                  view.account.label
                )
              }
            />
          ))}
        </div>
      )}
    </section>
  );
}
