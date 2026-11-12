import { useCallback, useEffect, useId, useRef, useState } from "react";
import type { CreditPackage, CreditUsageRow } from "../lib/types";
import { api, fmtCredits, fmtDateTime, quotaUnit } from "../lib/api";
import { CREDIT_GROUP_ORDER, groupLabel } from "../lib/credits";
import QuotaCard, { type QuotaCardProps } from "./QuotaCard";

const DAY_MS = 86_400_000;
const USAGE_URL = "https://www.workbuddy.cn/profile/plans-usage#usage-detail";

interface Props extends Omit<QuotaCardProps, "onGroup"> {
  accountId: string;
  accountLabel: string;
  providerId: string;
  providerName: string;
  onClose: () => void;
}

/** 单个分组的资源包列表（有效期内 / 已过期），首次激活时拉第一页，之后「加载更多」翻页。 */
function CreditPackages({
  accountId,
  kind,
  expired,
  active,
}: {
  accountId: string;
  kind: string;
  expired: boolean;
  active: boolean;
}) {
  const [rows, setRows] = useState<CreditPackage[]>([]);
  const [nextPage, setNextPage] = useState<number | null>(1);
  const [loading, setLoading] = useState(false);
  const [ready, setReady] = useState(false);
  const [failure, setFailure] = useState<{ page: number; error: string } | null>(null);
  const request = useRef(0);

  const load = useCallback(
    async (page: number) => {
      const version = ++request.current;
      setLoading(true);
      setFailure(null);
      try {
        const result = await api.workbuddyCreditPackages(accountId, kind, expired, page);
        if (request.current !== version) return;
        setRows((previous) => {
          const current = page === 1 ? [] : previous;
          const seen = new Set(current.map((r) => r.id).filter(Boolean));
          const items = result.items.filter((r) => {
            if (!r.id) return true;
            if (seen.has(r.id)) return false;
            seen.add(r.id);
            return true;
          });
          return [...current, ...items];
        });
        setReady(true);
        setNextPage(result.hasMore ? page + 1 : null);
      } catch (cause) {
        if (request.current === version) setFailure({ page, error: String(cause) });
      } finally {
        if (request.current === version) setLoading(false);
      }
    },
    [accountId, kind, expired]
  );

  const started = useRef<typeof load | null>(null);
  useEffect(
    () => () => {
      started.current = null;
      request.current++;
    },
    [load]
  );
  useEffect(() => {
    if (active && started.current !== load) {
      started.current = load;
      void load(1);
    }
  }, [active, load]);

  const now = Date.now();
  return (
    <div hidden={!active} className="credit-pkgs" aria-busy={loading}>
      {loading && !ready && <p className="credits-empty" role="status">正在读取资源包…</p>}
      {active && failure && (
        <>
          <p className="credits-error">加载失败：{failure.error}</p>
          <button className="btn small" onClick={() => void load(failure.page)}>
            重试
          </button>
        </>
      )}
      {rows.map((row, index) => {
        const days = row.expireAtMs > 0 ? (row.expireAtMs - now) / DAY_MS : 0;
        const expiring = !expired && row.remaining > 0 && days > 0 && days <= 3;
        return (
          <div className="credit-group" key={row.id || index}>
            <div className="credit-group-line">
              <strong>{groupLabel(kind)}</strong>
              <span className="credit-group-remaining" title={`剩余 ${fmtCredits(row.remaining)} 积分`}>
                剩余 {fmtCredits(row.remaining)}
              </span>
            </div>
            <div className="credit-group-line muted">
              <span>
                已使用 {fmtCredits(row.used)}/{fmtCredits(row.total)}
              </span>
              {row.expireAtMs > 0 && <span>到期 {fmtDateTime(row.expireAtMs)}</span>}
            </div>
            {row.name && <span className="muted credit-pkg-name">{row.name}</span>}
            {expiring && (
              <span className="credit-group-expiry">
                {days <= 1 ? "24 小时内到期" : `${Math.ceil(days)} 天内到期`}
              </span>
            )}
          </div>
        );
      })}
      {ready && rows.length === 0 && nextPage === null && !failure && (
        <div className="credits-empty">{expired ? "没有已过期的资源包" : "暂无资源包"}</div>
      )}
      {ready && !failure && nextPage !== null && (
        <button className="btn small" disabled={loading} onClick={() => void load(nextPage)}>
          {loading ? "加载中…" : "加载更多"}
        </button>
      )}
    </div>
  );
}

/** 近 30 天消耗明细（最近 5 条），首次切到该 tab 时拉取。 */
function CreditUsage({ accountId }: { accountId: string }) {
  const [attempt, setAttempt] = useState(0);
  const [rows, setRows] = useState<CreditUsageRow[] | null>(null);
  const [total, setTotal] = useState(0);
  const [error, setError] = useState("");
  const [linkError, setLinkError] = useState("");

  useEffect(() => {
    let live = true;
    setError("");
    api.workbuddyCreditUsage(accountId).then(
      (result) => {
        if (!live) return;
        setRows(result.items);
        setTotal(result.total);
      },
      (cause) => {
        if (live) setError(String(cause));
      }
    );
    return () => {
      live = false;
    };
  }, [accountId, attempt]);

  const credit2 = (n: number) =>
    Number.isFinite(n)
      ? n.toLocaleString("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })
      : "—";

  const openHistory = async () => {
    setLinkError("");
    try {
      await api.openExternal(USAGE_URL);
    } catch (cause) {
      setLinkError(String(cause));
    }
  };

  return (
    <div aria-busy={rows === null && !error}>
      {rows === null && !error && <p className="credits-empty" role="status">正在读取消耗记录…</p>}
      {(linkError || error) && <p className="credits-error">{linkError || `加载失败：${error}`}</p>}
      {error && (
        <button
          className="btn small"
          onClick={() => {
            setError("");
            setAttempt((v) => v + 1);
          }}
        >
          重试
        </button>
      )}
      <table className="set-table credits-usage">
        <thead>
          <tr>
            <th>请求 / 时间</th>
            <th className="r">消耗积分</th>
            <th>模型</th>
          </tr>
        </thead>
        <tbody>
          {rows?.length === 0 && (
            <tr>
              <td colSpan={3} className="credits-empty">
                近 30 天没有消耗记录
              </td>
            </tr>
          )}
          {rows?.map((row, index) => {
            const input = row.inputTrunc || row.input;
            return (
              <tr key={`${row.requestId}-${index}`}>
                <td>
                  <div className="credits-request" title={row.input || input || undefined}>
                    {input || "—"}
                  </div>
                  <div className="muted">{row.time || "—"}</div>
                </td>
                <td className="r">{credit2(row.credit)}</td>
                <td>{row.model || "—"}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <div className="credits-usage-foot">
        <span className="muted">
          近 30 天消耗{total > 0 ? `（共 ${total} 条，展示最近 ${rows?.length ?? 0} 条）` : ""}
        </span>
        <button className="btn small" onClick={() => void openHistory()}>
          查看更多 ↗
        </button>
      </div>
    </div>
  );
}

/**
 * 共用的账号额度窗口。WorkBuddy 另有官方资源包与消耗记录，按需读取并保留分页。
 */
export default function CreditsDialog({
  accountId, accountLabel, providerId, providerName, onClose, ...quotaProps
}: Props) {
  const titleId = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const [group, setGroup] = useState<string | null>(null);
  const [expired, setExpired] = useState(false);
  const [usage, setUsage] = useState(false);
  const [usageOpened, setUsageOpened] = useState(false);
  const [openedGroups, setOpenedGroups] = useState<string[]>([]);

  const workbuddy = providerId === "workbuddy";
  const title = quotaUnit(providerId) === "Token" ? "额度明细" : "积分明细";
  const groups = quotaProps.quota?.creditGroups ?? [];

  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  useEffect(() => {
    if (body.current) body.current.scrollTop = 0;
  }, [group, expired, usage]);

  useEffect(() => {
    heading.current?.focus({ preventScroll: true });
  }, [group]);

  const openGroup = (kind: string) => {
    setExpired(false);
    setGroup(kind);
    setOpenedGroups((prev) => (prev.includes(kind) ? prev : [...prev, kind]));
  };

  const close = () => {
    dialog.current?.close();
    onClose();
  };

  const missingGroups = CREDIT_GROUP_ORDER.filter((k) => !groups.some((g) => g.kind === k));

  return (
    <dialog
      className="credits-dialog"
      ref={dialog}
      aria-labelledby={titleId}
      onCancel={(event) => {
        event.preventDefault();
        close();
      }}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <div className="credits-head">
        <div className="credits-heading">
          <h2 id={titleId} ref={heading} tabIndex={-1}>{group ? groupLabel(group) : title}</h2>
          <span className="muted">{providerName} · {accountLabel}</span>
        </div>
        <button className="credits-close" aria-label="关闭弹窗" onClick={close}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden>
            <path d="M18 6 6 18M6 6l12 12" />
          </svg>
        </button>
      </div>

      <div className="credits-body" ref={body}>
        {!workbuddy ? (
          <QuotaCard {...quotaProps} providerId={providerId} />
        ) : group ? (
          <div className="set-tabs credits-tabs" role="group" aria-label="资源包状态">
            {([false, true] as const).map((value) => (
              <button
                key={String(value)}
                className={`set-tab ${expired === value ? "active" : ""}`}
                aria-pressed={expired === value}
                onClick={() => setExpired(value)}
              >
                {value ? "已过期" : "有效期内"}
              </button>
            ))}
          </div>
        ) : (
          <>
            <div className="set-tabs credits-tabs" role="group" aria-label="积分明细">
              {([false, true] as const).map((showUsage) => (
                <button
                  key={String(showUsage)}
                  className={`set-tab ${usage === showUsage ? "active" : ""}`}
                  aria-pressed={usage === showUsage}
                  onClick={() => {
                    setUsage(showUsage);
                    if (showUsage) setUsageOpened(true);
                  }}
                >
                  {showUsage ? "消耗明细" : "积分概览"}
                </button>
              ))}
            </div>
            {!usage && (
              <>
                <QuotaCard {...quotaProps} providerId={providerId} onGroup={openGroup} />
                {missingGroups.length > 0 && (
                  <div className="credits-missing">
                    {missingGroups.map((kind) => (
                      <button key={kind} className="btn small" onClick={() => openGroup(kind)}>
                        {groupLabel(kind)} ›
                      </button>
                    ))}
                  </div>
                )}
              </>
            )}
          </>
        )}

        {usageOpened && (
          <div hidden={group != null || !usage}>
            <CreditUsage accountId={accountId} />
          </div>
        )}
        {openedGroups.flatMap((kind) =>
          ([false, true] as const).map((isExpired) => (
            <CreditPackages
              key={`${kind}-${isExpired}`}
              accountId={accountId}
              kind={kind}
              expired={isExpired}
              active={group === kind && expired === isExpired}
            />
          ))
        )}
      </div>

      <div className="credits-foot">
        {group && (
          <button className="btn small" onClick={() => setGroup(null)}>
            返回概览
          </button>
        )}
        <button className="btn small" onClick={close}>
          关闭
        </button>
      </div>
    </dialog>
  );
}
