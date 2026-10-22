export type ClaimState = "claimable" | "claimed" | "notAvailable" | "unknown";

export interface CheckinStatus {
  state: ClaimState;
  points: number;
  streakDays: number;
  message: string;
  dayIndex?: number | null;
  bonusPoints: number;
}

export interface CreditItem {
  creditType: string;
  granted: number;
  consumed: number;
  remaining: number;
  grantedAtMs: number;
  expireAtMs: number;
}

export interface Quota {
  totalGranted: number;
  totalConsumed: number;
  totalRemaining: number;
  items: CreditItem[];
  message: string;
}

export interface Account {
  id: string;
  provider: string;
  label: string;
  createdAt: number;
  lastUsed?: number | null;
  origin: string;
}

export interface AccountView {
  account: Account;
  providerName: string;
  accent: string;
  status?: CheckinStatus | null;
  quota?: Quota | null;
  quotaError?: string | null;
  error?: string | null;
  todayLog?: LogEntry | null;
  /** 是否为该 agent 的当前账号 */
  isActive: boolean;
  /** 客户端数据目录 */
  dataDir?: string | null;
  projectCount: number;
  sessionCount: number;
  /** 积分是否低于预警阈值 */
  isLow: boolean;
  /** 积分预警阈值 */
  alertThreshold: number;
  /** 最近 7 天积分快照（升序） */
  trend: QuotaPoint[];
}

export interface QuotaPoint {
  timestamp: string;
  remaining: number;
}

export interface ProviderView {
  id: string;
  name: string;
  accent: string;
  docsUrl: string;
  supportsDiscovery: boolean;
  configured: boolean;
  hasClient: boolean;
  clientExe?: string | null;
}

/** 单个 agent 自己的自动签到配置 */
export interface AgentScheduleView {
  enabled: boolean;
  startMinute: number;
  endMinute: number;
  accountCount: number;
  todayClaimed: number;
  todayClaimable: number;
  alertThreshold?: number;
}

export interface Overview {
  accounts: AccountView[];
  providers: ProviderView[];
  todayPoints: number;
  claimedCount: number;
  totalCount: number;
  /** 总闸 */
  schedulerEnabled: boolean;
  /** 每个 agent 自己的调度配置 */
  agents: Record<string, AgentScheduleView>;
  /** 每个账号今天的计划执行时刻（分钟） */
  plans: Record<string, number>;
}

export interface AgentGroup {
  provider: ProviderView;
  accounts: AccountView[];
  totalRemaining: number;
  totalGranted: number;
  claimableCount: number;
  claimedToday: number;
  schedule: AgentScheduleView;
}

export interface AccountResult {
  accountId: string;
  provider: string;
  label: string;
  status: "success" | "already" | "failed" | "skipped";
  points: number;
  message: string;
}

export interface LogEntry {
  id: string;
  timestamp: string;
  date: string;
  provider: string;
  accountId: string;
  label: string;
  status: string;
  points: number;
  message: string;
}

export interface AgentScheduleConfig {
  enabled: boolean;
  startMinute: number;
  endMinute: number;
}

export interface SchedulerConfig {
  enabled: boolean;
  agents: Record<string, AgentScheduleConfig>;
}

export interface AppConfig {
  scheduler: SchedulerConfig;
  /** 每个 agent 当前激活的账号 id */
  activeAccounts: Record<string, string>;
  alertThresholds: Record<string, number>;
}

export interface DiscoveredAccount {
  account: Account;
  provider: string;
  alreadyAdded: boolean;
}

export interface AuthChallenge {
  sessionId: string;
  authUrl: string;
}
