/**
 * TypeScript types matching the Zerobase (PocketBase-compatible) API responses.
 *
 * All types use camelCase field names to match the `#[serde(rename_all = "camelCase")]`
 * serialization used by the Rust backend.
 */

// ── Pagination ────────────────────────────────────────────────────────────────

/** Paginated list response matching PocketBase format. */
export interface ListResponse<T> {
  page: number;
  perPage: number;
  totalPages: number;
  totalItems: number;
  items: T[];
}

// ── Records ───────────────────────────────────────────────────────────────────

/** Base record fields present on every record. */
export interface BaseRecord {
  id: string;
  collectionId: string;
  collectionName: string;
  created: string;
  updated: string;
  [key: string]: unknown;
}

/** Auth record extends base with authentication fields. */
export interface AuthRecord extends BaseRecord {
  email: string;
  emailVisibility: boolean;
  verified: boolean;
}

// ── Collections ───────────────────────────────────────────────────────────────

export type CollectionType = 'base' | 'auth' | 'view';

export type IndexSortDirection = 'asc' | 'desc';

export interface IndexColumn {
  name: string;
  direction: IndexSortDirection;
}

export interface IndexSpec {
  columns: string[];
  indexColumns: IndexColumn[];
  unique: boolean;
}

export interface ApiRules {
  listRule: string | null;
  viewRule: string | null;
  createRule: string | null;
  updateRule: string | null;
  deleteRule: string | null;
  manageRule?: string | null;
}

export interface AuthOptions {
  allowEmailAuth: boolean;
  allowOauth2Auth: boolean;
  allowOtpAuth: boolean;
  requireEmail: boolean;
  mfaEnabled: boolean;
  mfaDuration: number;
  minPasswordLength: number;
  identityFields: string[];
  manageRule?: string | null;
}

/** Field type discriminator values. */
export type FieldTypeName =
  | 'text'
  | 'number'
  | 'bool'
  | 'email'
  | 'url'
  | 'dateTime'
  | 'select'
  | 'multiSelect'
  | 'autoDate'
  | 'file'
  | 'relation'
  | 'json'
  | 'editor';

export interface Field {
  id: string;
  name: string;
  type: FieldType;
  required: boolean;
  unique: boolean;
  sortOrder: number;
}

/**
 * Tagged union for field types. Uses adjacently-tagged format to match the
 * Rust `FieldType` enum with `#[serde(tag = "type", content = "options")]`.
 */
export type FieldType =
  | { type: 'text'; options: { minLength: number; maxLength: number; pattern: string | null; searchable: boolean } }
  | { type: 'number'; options: { min: number | null; max: number | null; noDecimal: boolean } }
  | { type: 'bool'; options: Record<string, never> }
  | { type: 'email'; options: { exceptDomains: string[]; onlyDomains: string[] } }
  | { type: 'url'; options: { exceptDomains: string[]; onlyDomains: string[] } }
  | { type: 'dateTime'; options: { min: string; max: string } }
  | { type: 'select'; options: { values: string[] } }
  | { type: 'multiSelect'; options: { values: string[]; maxSelect: number } }
  | { type: 'autoDate'; options: { onCreate: boolean; onUpdate: boolean } }
  | { type: 'file'; options: { maxSize: number; maxSelect: number; mimeTypes: string[]; thumbs: string[] } }
  | { type: 'relation'; options: { collectionId: string; cascadeDelete: boolean; maxSelect: number | null } }
  | { type: 'json'; options: { maxSize: number } }
  | { type: 'editor'; options: { maxLength: number; searchable: boolean } };

export interface Collection {
  id: string;
  name: string;
  type: CollectionType;
  fields: Field[];
  rules: ApiRules;
  indexes?: IndexSpec[];
  viewQuery?: string | null;
  authOptions?: AuthOptions | null;
}

// ── Auth ──────────────────────────────────────────────────────────────────────

export interface AuthWithPasswordRequest {
  identity: string;
  password: string;
}

export interface AuthResponse {
  token: string;
  record: AuthRecord;
}

export interface AdminAuthResponse {
  token: string;
  admin: AuthRecord;
}

export interface AuthRefreshResponse {
  token: string;
  record: AuthRecord;
}

// ── Settings ──────────────────────────────────────────────────────────────────

/** Server settings as a JSON object with category keys. */
export type Settings = Record<string, Record<string, unknown>>;

/** SMTP settings shape (matches the Rust SmtpSettingsDto). */
export interface SmtpSettings {
  enabled: boolean;
  host: string;
  port: number;
  username: string;
  /** Write-only — reads return an empty string. */
  password: string;
  tls: boolean;
}

/** Sender settings from the meta category. */
export interface MetaSenderSettings {
  appName: string;
  appUrl: string;
  senderName: string;
  senderAddress: string;
}

/** S3 storage settings (matches the Rust S3SettingsDto). */
export interface S3Settings {
  enabled: boolean;
  bucket: string;
  region: string;
  endpoint: string;
  accessKey: string;
  /** Write-only — reads return an empty string. */
  secretKey: string;
  forcePathStyle: boolean;
}

/** Request body for sending a test email. */
export interface TestEmailRequest {
  to: string;
}

/** Response from the test-email endpoint. */
export interface TestEmailResponse {
  success: boolean;
}

// ── Logs ──────────────────────────────────────────────────────────────────────

export interface LogEntry {
  id: string;
  method: string;
  url: string;
  status: number;
  ip: string;
  authId: string;
  durationMs: number;
  userAgent: string;
  requestId: string;
  created: string;
}

/** Request counts by HTTP status category. */
export interface StatusCounts {
  success: number;
  redirect: number;
  clientError: number;
  serverError: number;
}

/** A single data point in the timeline series. */
export interface TimelineEntry {
  date: string;
  total: number;
}

/** Aggregate log statistics. */
export interface LogStats {
  totalRequests: number;
  statusCounts: StatusCounts;
  avgDurationMs: number;
  maxDurationMs: number;
  timeline: TimelineEntry[];
}

export interface ListLogsParams {
  method?: string;
  url?: string;
  statusMin?: number;
  statusMax?: number;
  authId?: string;
  ip?: string;
  createdAfter?: string;
  createdBefore?: string;
  filter?: string;
  page?: number;
  perPage?: number;
  sort?: string;
}

export interface LogStatsParams {
  createdAfter?: string;
  createdBefore?: string;
  groupBy?: string;
}

// ── Backups ───────────────────────────────────────────────────────────────────

export interface BackupEntry {
  name: string;
  size: number;
  created: string;
}

// ── Records query params ──────────────────────────────────────────────────────

export interface ListRecordsParams {
  page?: number;
  perPage?: number;
  sort?: string;
  filter?: string;
  fields?: string;
  search?: string;
  expand?: string;
}

// ── Errors ────────────────────────────────────────────────────────────────────

export interface FieldError {
  code: string;
  message: string;
}

/** Error response body matching PocketBase format. */
export interface ErrorResponseBody {
  code: number;
  message: string;
  data: Record<string, FieldError>;
}

// ── OTP ───────────────────────────────────────────────────────────────────────

export interface RequestOtpRequest {
  email: string;
}

export interface RequestOtpResponse {
  otpId: string;
}

export interface AuthWithOtpRequest {
  otpId: string;
  password: string;
}

// ── MFA ───────────────────────────────────────────────────────────────────────

export interface MfaSetupResponse {
  secret: string;
  qrCodeUrl: string;
}

export interface ConfirmMfaRequest {
  code: string;
}

export interface AuthWithMfaRequest {
  mfaId: string;
  code: string;
}

// ── OAuth2 Provider Settings (Admin) ──────────────────────────────────────────

/** OAuth2 provider configuration stored under `auth.oauth2Providers`. */
export interface OAuth2ProviderSettings {
  enabled: boolean;
  clientId: string;
  /** Write-only — reads return an empty string. */
  clientSecret: string;
  authUrl?: string;
  tokenUrl?: string;
  userInfoUrl?: string;
  displayName: string;
}

/** Auth settings from the `auth` settings category. */
export interface AuthSettingsDto {
  tokenDuration: number;
  refreshTokenDuration: number;
  allowEmailAuth: boolean;
  allowOauth2Auth: boolean;
  allowMfa: boolean;
  allowOtpAuth: boolean;
  allowPasskeyAuth: boolean;
  minPasswordLength: number;
  oauth2Providers: Record<string, OAuth2ProviderSettings>;
}

// ── OAuth2 ────────────────────────────────────────────────────────────────────

export interface AuthMethodsResponse {
  emailPassword: boolean;
  otp: boolean;
  mfa: boolean;
  authProviders: AuthProvider[];
}

export interface AuthProvider {
  name: string;
  displayName: string;
  state: string;
  authUrl: string;
  codeVerifier: string;
  codeChallenge: string;
  codeChallengeMethod: string;
}

export interface AuthWithOAuth2Request {
  provider: string;
  code: string;
  codeVerifier: string;
  redirectUrl: string;
}

// ── External Auths ────────────────────────────────────────────────────────────

export interface ExternalAuth {
  id: string;
  collectionId: string;
  recordId: string;
  provider: string;
  providerId: string;
  created: string;
  updated: string;
}

// ── File Token ────────────────────────────────────────────────────────────────

export interface FileTokenResponse {
  token: string;
}

// ── Verification / Password Reset / Email Change ──────────────────────────────

export interface RequestVerificationRequest {
  email: string;
}

export interface ConfirmVerificationRequest {
  token: string;
}

export interface RequestPasswordResetRequest {
  email: string;
}

export interface ConfirmPasswordResetRequest {
  token: string;
  password: string;
  passwordConfirm: string;
}

export interface RequestEmailChangeRequest {
  newEmail: string;
}

export interface ConfirmEmailChangeRequest {
  token: string;
  password: string;
}

// ── Webhooks ─────────────────────────────────────────────────────────────────

/** Events that can trigger a webhook delivery. */
export type WebhookEvent = 'create' | 'update' | 'delete';

/** A webhook configuration attached to a collection. */
export interface Webhook {
  id: string;
  collection: string;
  url: string;
  events: WebhookEvent[];
  secret?: string;
  enabled: boolean;
  created: string;
  updated: string;
}

/** Input for creating a new webhook. */
export interface CreateWebhookInput {
  collection: string;
  url: string;
  events: WebhookEvent[];
  secret?: string;
  enabled?: boolean;
}

/** Input for updating an existing webhook. */
export interface UpdateWebhookInput {
  url?: string;
  events?: WebhookEvent[];
  secret?: string;
  enabled?: boolean;
}

/** Delivery status of a webhook invocation. */
export type WebhookDeliveryStatus = 'success' | 'failed' | 'pending';

/** A log entry for a single webhook delivery attempt. */
export interface WebhookDeliveryLog {
  id: string;
  webhookId: string;
  event: WebhookEvent;
  collection: string;
  recordId: string;
  url: string;
  responseStatus: number;
  attempt: number;
  status: WebhookDeliveryStatus;
  error?: string;
  created: string;
}

/** Response from the test-webhook endpoint. */
export interface TestWebhookResponse {
  success: boolean;
  statusCode: number;
  error?: string;
}

// ── Batch ─────────────────────────────────────────────────────────────────────

export interface BatchRequest {
  requests: BatchOperation[];
}

export interface BatchOperation {
  method: 'GET' | 'POST' | 'PATCH' | 'DELETE';
  url: string;
  body?: unknown;
}

export interface BatchResponse {
  responses: BatchOperationResponse[];
}

export interface BatchOperationResponse {
  status: number;
  body: unknown;
}
