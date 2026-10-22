export type ClaimState = "claimable" | "claimed" | "notAvailable" | "unknown";

export interface CheckinStatus {
  state: ClaimState;
  points: number;
  streakDays: number;
  message: string;
  dayIndex?: number | null;
  bonusPoints: number;
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
  error?: string | null;
  todayLog?: LogEntry | null;
}

export interface ProviderView {
  id: string;
  name: string;
  accent: string;
  docsUrl: string;
  supportsDiscovery: boolean;
  configured: boolean;
}

export interface SchedulerView {
  enabled: boolean;
  startMinute: number;
  endMinute: number;
  providerEnabled: Record<string, boolean>;
  plans: Record<string, number>;
}

export interface Overview {
  accounts: AccountView[];
  providers: ProviderView[];
  todayPoints: number;
  claimedCount: number;
  totalCount: number;
  scheduler: SchedulerView;
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

export interface SchedulerConfig {
  enabled: boolean;
  startMinute: number;
  endMinute: number;
  providers: Record<string, boolean>;
}

export interface AppConfig {
  scheduler: SchedulerConfig;
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
