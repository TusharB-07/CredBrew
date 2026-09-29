import { AuthResponse, Event, ParticipantType, Participant, Template, VerifyResponse, IssueResponse, IssueStatusResponse, ImportResult, PaginatedResponse, ParticipantWithCredentialStatus, NameZone, QrZone } from '../types';

export type {
  AuthResponse,
  User,
  Event,
  ParticipantType,
  Participant,
  Template,
  Credential,
  VerifyResponse,
  IssueResponse,
  IssueStatusResponse,
  ImportResult,
  PaginatedResponse,
  ParticipantWithCredentialStatus,
  NameZone,
  QrZone,
} from '../types';

const API_BASE = '/api';

let authToken: string | null = null;

export function setAuthToken(token: string | null) {
  authToken = token;
  if (token) {
    localStorage.setItem('auth_token', token);
  } else {
    localStorage.removeItem('auth_token');
  }
}

export function getAuthToken(): string | null {
  if (!authToken) {
    authToken = localStorage.getItem('auth_token');
  }
  return authToken;
}

async function fetchWithAuth<T>(path: string, options: RequestInit = {}): Promise<T> {
  const token = getAuthToken();
  const headers: HeadersInit = {
    'Content-Type': 'application/json',
    ...options.headers,
  };

  if (token) {
    (headers as Record<string, string>)['Authorization'] = `Bearer ${token}`;
  }

  const response = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers,
  });

  if (response.status === 401) {
    setAuthToken(null);
    window.location.href = '/login';
    throw new Error('Unauthorized');
  }

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: 'Request failed' }));
    throw new Error(error.error || `HTTP ${response.status}`);
  }

  if (response.status === 204) {
    return {} as T;
  }

  return response.json();
}

async function fetchMultipart<T>(path: string, formData: FormData): Promise<T> {
  const token = getAuthToken();
  const headers: HeadersInit = {};
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }

  const response = await fetch(`${API_BASE}${path}`, {
    method: 'POST',
    headers,
    body: formData,
  });

  if (response.status === 401) {
    setAuthToken(null);
    window.location.href = '/login';
    throw new Error('Unauthorized');
  }

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: 'Request failed' }));
    throw new Error(error.error || `HTTP ${response.status}`);
  }

  return response.json();
}

// Auth
export async function register(name: string, email: string, password: string): Promise<AuthResponse> {
  return fetchWithAuth('/auth/register', {
    method: 'POST',
    body: JSON.stringify({ name, email, password }),
  });
}

export async function login(email: string, password: string): Promise<AuthResponse> {
  return fetchWithAuth('/auth/login', {
    method: 'POST',
    body: JSON.stringify({ email, password }),
  });
}

// Events
export async function createEvent(data: { name: string; event_date?: string; org_name?: string; accent_color?: string }): Promise<Event> {
  return fetchWithAuth('/events', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function listEvents(): Promise<Event[]> {
  return fetchWithAuth('/events');
}

export async function getEvent(id: string): Promise<Event> {
  return fetchWithAuth(`/events/${id}`);
}

export async function updateEvent(id: string, data: Partial<Event>): Promise<Event> {
  return fetchWithAuth(`/events/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(data),
  });
}

// Participant Types
export async function createParticipantType(eventId: string, data: { name: string; badge_enabled: boolean; certificate_enabled: boolean }): Promise<ParticipantType> {
  return fetchWithAuth(`/events/${eventId}/types`, {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function updateParticipantType(id: string, data: Partial<ParticipantType>): Promise<ParticipantType> {
  return fetchWithAuth(`/types/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(data),
  });
}

export async function deleteParticipantType(id: string): Promise<void> {
  return fetchWithAuth(`/types/${id}`, {
    method: 'DELETE',
  });
}

// Participants
export async function createParticipant(eventId: string, data: { name: string; email: string; participant_type_id: string }): Promise<Participant> {
  return fetchWithAuth(`/events/${eventId}/participants`, {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function listParticipants(eventId: string, params?: { page?: number; per_page?: number; type_id?: string }): Promise<PaginatedResponse<ParticipantWithCredentialStatus>> {
  const searchParams = new URLSearchParams();
  if (params?.page) searchParams.set('page', params.page.toString());
  if (params?.per_page) searchParams.set('per_page', params.per_page.toString());
  if (params?.type_id) searchParams.set('type_id', params.type_id);
  return fetchWithAuth(`/events/${eventId}/participants?${searchParams}`);
}

export async function importParticipants(eventId: string, file: File): Promise<ImportResult> {
  const formData = new FormData();
  formData.append('file', file);
  return fetchMultipart(`/events/${eventId}/participants/import`, formData);
}

export async function deleteParticipant(id: string): Promise<void> {
  return fetchWithAuth(`/participants/${id}`, {
    method: 'DELETE',
  });
}

// Templates
export async function createTemplate(data: {
  participant_type_id: string;
  kind: 'badge' | 'certificate';
  file: File;
  name_zone: NameZone;
  qr_zone: QrZone;
}): Promise<Template> {
  const formData = new FormData();
  formData.append('participant_type_id', data.participant_type_id);
  formData.append('kind', data.kind);
  formData.append('file', data.file);
  formData.append('name_zone', JSON.stringify(data.name_zone));
  formData.append('qr_zone', JSON.stringify(data.qr_zone));
  return fetchMultipart('/templates', formData);
}

export async function previewTemplate(id: string): Promise<Blob> {
  const token = getAuthToken();
  const headers: HeadersInit = {};
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }
  const response = await fetch(`${API_BASE}/templates/${id}/preview`, { headers });
  if (!response.ok) throw new Error('Failed to fetch preview');
  return response.blob();
}

export async function getTemplateFile(id: string): Promise<string> {
  const token = getAuthToken();
  const headers: HeadersInit = {};
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }
  const response = await fetch(`${API_BASE}/templates/${id}/file`, { headers, redirect: 'manual' });
  if (response.status === 302) {
    return response.headers.get('Location') || '';
  }
  throw new Error('Failed to get template file');
}

// Issue
export async function issueCredentials(eventId: string, data: {
  participant_type_id: string;
  kind: 'badge' | 'certificate' | 'both';
  send_email: boolean;
}): Promise<IssueResponse> {
  return fetchWithAuth(`/events/${eventId}/issue`, {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function getIssueStatus(eventId: string, params?: { type_id?: string; kind?: string }): Promise<IssueStatusResponse> {
  const searchParams = new URLSearchParams();
  if (params?.type_id) searchParams.set('type_id', params.type_id);
  if (params?.kind) searchParams.set('kind', params.kind);
  return fetchWithAuth(`/events/${eventId}/issue-status?${searchParams}`);
}

export async function retryCredential(id: string): Promise<void> {
  return fetchWithAuth(`/credentials/${id}/retry`, {
    method: 'POST',
  });
}

export async function revokeCredential(id: string): Promise<void> {
  return fetchWithAuth(`/credentials/${id}`, {
    method: 'PATCH',
    body: JSON.stringify({ status: 'revoked' }),
  });
}

// Public
export async function verifyCredential(code: string): Promise<VerifyResponse> {
  return fetchWithAuth(`/public/verify/${code}`);
}