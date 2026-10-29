export type ClaimState = "claimable" | "claimed" | "notAvailable" | "unknown";

/** 点窗口右上角 × 时的行为 */
export type CloseAction = "ask" | "tray" | "exit";

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
  /** 客户端当前实际登录的是不是这个账号 */
  isClientLogin: boolean;
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
  /** 支持浏览器授权导入账号（设备码 / OAuth 页） */
  supportsOAuth: boolean;
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
  /** 该 agent 的数据自动同步间隔（分钟） */
  refreshIntervalMinutes: number;
  /** 客户端当前实际登录的账号（读不到就是 undefined） */
  clientAccount?: ClientAccountView;
  /** 能不能靠「写凭据文件」给这个客户端换账号 */
  switchable: boolean;
  /** 换号时是否同步客户端本地聊天记录 */
  chatSyncEnabled: boolean;
  /** 这个平台的客户端是否支持聊天记录同步（决定开关显不显示） */
  chatSyncSupported: boolean;
}

/** 客户端当前登录的账号 */
export interface ClientAccountView {
  accountId: string;
  label: string;
  /** 这个账号在不在我们的账号池里 */
  inPool: boolean;
}

/** 切换客户端账号的结果 */
export interface SwitchOutcome {
  /** 客户端原登录态的备份路径 */
  backup?: string;
  /** 写入的凭据文件 */
  targetFile: string;
  clientExe: string;
  /** 聊天记录同步结果（开关没开就是 undefined） */
  chatSync?: {
    action: string;
    detail: string;
    files: number;
    bytes: number;
    added: number;
    replaced: number;
  };
}

export interface Overview {
  accounts: AccountView[];
  providers: ProviderView[];
  todayPoints: number;
  claimedCount: number;
  totalCount: number;
  /** 签到总闸 */
  schedulerEnabled: boolean;
  /** 自动同步（积分 / 状态 / 资料）总开关 */
  syncEnabled: boolean;
  /** 每个 agent 自己的调度配置 */
  agents: Record<string, AgentScheduleView>;
  /** 每个账号今天的计划执行时刻（分钟） */
  plans: Record<string, number>;
  /** 应用数据目录 */
  dataDir: string;
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

/** 首页的虚拟 id（不是真实 provider）。 */
export const HOME_ID = "__home__";

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
  /** 该 agent 的数据自动同步间隔（分钟） */
  refreshIntervalMinutes: number;
  /** 换号时是否同步客户端本地聊天记录 */
  chatSyncEnabled?: boolean;
}

/** 一条换号聊天记录备份（设置页「历史备份」） */
export interface ChatBackupEntry {
  /** workbuddy / qoder */
  provider: string;
  /** 操作 id（`<毫秒时间戳>-<进程号>`） */
  opId: string;
  /** 备份时间（毫秒时间戳；0 = 解析不出来） */
  createdAtMs: number;
  files: number;
  bytes: number;
  /** 顶层内容（main.sqlite / history 等） */
  items: string[];
}

export interface SchedulerConfig {
  enabled: boolean;
  /** 自动同步（积分 / 签到状态 / 账号资料）总开关 */
  syncEnabled: boolean;
  agents: Record<string, AgentScheduleConfig>;
}

export interface AppConfig {
  scheduler: SchedulerConfig;
  /** 每个 agent 当前激活的账号 id */
  activeAccounts: Record<string, string>;
  alertThresholds: Record<string, number>;
  /** 关闭主窗口时的行为（默认每次询问） */
  closeAction: CloseAction;
}

export interface DiscoveredAccount {
  account: Account;
  provider: string;
  alreadyAdded: boolean;
}

export interface AuthChallenge {
  sessionId: string;
  authUrl: string;
  /** 发起授权的 provider */
  provider?: string;
}
