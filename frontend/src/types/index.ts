export interface User {
  id: string;
  email: string;
  name: string;
}

export interface AuthResponse {
  token: string;
  user: User;
}

export interface Event {
  id: string;
  owner_id: string;
  name: string;
  event_date: string | null;
  org_name: string;
  accent_color: string;
  created_at: string;
  total_participants?: number;
  credentials_emailed?: number;
  credentials_failed?: number;
  participant_types?: ParticipantTypeWithCounts[];
}

export interface ParticipantTypeWithCounts extends ParticipantType {
  participant_count: number;
  badge_template: boolean;
  certificate_template: boolean;
  badges_issued: number;
  certificates_issued: number;
}

export interface ParticipantType {
  id: string;
  event_id: string;
  name: string;
  badge_enabled: boolean;
  certificate_enabled: boolean;
}

export interface Participant {
  id: string;
  event_id: string;
  participant_type_id: string;
  name: string;
  email: string;
  short_code: string;
  created_at: string;
}

export interface Template {
  id: string;
  participant_type_id: string;
  kind: 'badge' | 'certificate';
  file_key: string;
  name_zone: NameZone;
  qr_zone: QrZone;
}

export interface NameZone {
  x_pct: number;
  y_pct: number;
  w_pct: number;
  font_size_pct: number;
  color: string;
  align: string;
}

export interface QrZone {
  x_pct: number;
  y_pct: number;
  size_pct: number;
}

export interface Credential {
  id: string;
  participant_id: string;
  template_id: string;
  kind: 'badge' | 'certificate';
  file_key: string | null;
  status: 'pending' | 'rendered' | 'emailed' | 'failed' | 'revoked';
  attempts: number;
  last_error: string | null;
  issued_at: string;
  updated_at: string;
}

export interface VerifyResponse {
  verified: boolean;
  name: string;
  participant_type: string;
  event_name: string;
  org_name: string;
  issued_at: string;
  credential_image_url: string;
  download_url: string;
  linkedin_add_url: string;
  linkedin_share_url: string;
}

export interface IssueResponse {
  queued: number;
  skipped: number;
}

export interface IssueStatusResponse {
  pending: number;
  rendered: number;
  emailed: number;
  failed: number;
  total: number;
  recent_failures: FailureDetail[];
}

export interface FailureDetail {
  credential_id: string;
  participant_name: string;
  participant_email: string;
  error: string;
  attempts: number;
  updated_at: string;
}

export interface ImportResult {
  imported: number;
  errors: ImportError[];
}

export interface ImportError {
  row: number;
  field: string;
  message: string;
}

export interface PaginatedResponse<T> {
  participants: T[];
  total: number;
  page: number;
  per_page: number;
}

export interface ParticipantWithCredentialStatus extends Participant {
  badge_status: string | null;
  certificate_status: string | null;
}