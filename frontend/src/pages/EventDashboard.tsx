import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { getEvent, createParticipantType, updateParticipantType, deleteParticipantType, Event, ParticipantType } from '../api/client';
import { format } from 'date-fns';

interface ParticipantTypeWithCounts extends ParticipantType {
  participant_count: number;
  badge_template: boolean;
  certificate_template: boolean;
  badges_issued: number;
  certificates_issued: number;
}

interface EventWithStats extends Event {
  participant_types: ParticipantTypeWithCounts[];
  total_participants: number;
  credentials_emailed: number;
  credentials_failed: number;
}

export default function EventDashboard() {
  const { id } = useParams<{ id: string }>();
  const queryClient = useQueryClient();
  const [showAddType, setShowAddType] = useState(false);
  const [newTypeName, setNewTypeName] = useState('');
  const [newBadge, setNewBadge] = useState(true);
  const [newCert, setNewCert] = useState(false);
  const [typeError, setTypeError] = useState<string | null>(null);

  const { data: event, isLoading, error } = useQuery({
    queryKey: ['event', id],
    queryFn: () => getEvent(id!),
    enabled: !!id,
  });

  const { mutate: createType, isPending: createPending } = useMutation({
    mutationFn: (data: { name: string; badge_enabled: boolean; certificate_enabled: boolean }) =>
      createParticipantType(id!, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['event', id] });
      setShowAddType(false);
      setNewTypeName('');
      setNewBadge(true);
      setNewCert(false);
      setTypeError(null);
    },
    onError: (err) => setTypeError(err instanceof Error ? err.message : 'Failed to create participant type'),
  });

  const { mutate: updateType } = useMutation({
    mutationFn: (data: { ptId: string; badge_enabled?: boolean; certificate_enabled?: boolean }) =>
      updateParticipantType(data.ptId, { badge_enabled: data.badge_enabled, certificate_enabled: data.certificate_enabled }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['event', id] });
      setTypeError(null);
    },
    onError: (err) => setTypeError(err instanceof Error ? err.message : 'Failed to update participant type'),
  });

  const { mutate: deleteType } = useMutation({
    mutationFn: deleteParticipantType,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['event', id] });
      setTypeError(null);
    },
    onError: (err) => setTypeError(err instanceof Error ? err.message : 'Failed to delete participant type'),
  });

  const handleCreateType = () => {
    const name = newTypeName.trim();
    if (!name) {
      setTypeError('Type name is required');
      return;
    }
    createType({ name, badge_enabled: newBadge, certificate_enabled: newCert });
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="animate-spin rounded-full h-12 w-12 border-4 border-primary-600 border-t-transparent"></div>
      </div>
    );
  }

  if (error || !event) {
    return (
      <div className="text-center py-12">
        <p className="text-red-600">Failed to load event: {error?.message}</p>
      </div>
    );
  }

  const typedEvent = event as EventWithStats;

  const badgeToggle = (pt: ParticipantTypeWithCounts) => (
    <input
      type="checkbox"
      checked={pt.badge_enabled}
      onChange={() => updateType({ ptId: pt.id, badge_enabled: !pt.badge_enabled })}
      className="sr-only peer"
    />
  );

  const certToggle = (pt: ParticipantTypeWithCounts) => (
    <input
      type="checkbox"
      checked={pt.certificate_enabled}
      onChange={() => updateType({ ptId: pt.id, certificate_enabled: !pt.certificate_enabled })}
      className="sr-only peer"
    />
  );

  return (
    <div>
      <div className="mb-8">
        <Link to="/events" className="text-sm text-gray-500 hover:text-gray-700 mb-4 inline-block">
          ← Back to Events
        </Link>
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-3xl font-bold text-gray-900">{typedEvent.name}</h1>
            <div className="flex items-center gap-4 mt-2 text-sm text-gray-500">
              {typedEvent.org_name && <span>{typedEvent.org_name}</span>}
              {typedEvent.event_date && (
                <span>📅 {format(new Date(typedEvent.event_date), 'MMMM d, yyyy')}</span>
              )}
              <span className="w-3 h-3 rounded-full" style={{ backgroundColor: typedEvent.accent_color }}></span>
            </div>
          </div>
          <div className="flex gap-2">
            <Link to={`/events/${id}/participants`} className="btn-secondary">Manage Participants</Link>
            <Link to={`/events/${id}/templates`} className="btn-secondary">Templates</Link>
            <Link to={`/events/${id}/issue`} className="btn-primary">Issue & Send</Link>
          </div>
        </div>
      </div>

      <div className="grid gap-6 md:grid-cols-3 mb-8">
        <div className="card p-6">
          <h3 className="text-sm font-medium text-gray-500">Total Participants</h3>
          <p className="mt-1 text-3xl font-bold text-gray-900">{typedEvent.total_participants || 0}</p>
        </div>
        <div className="card p-6">
          <h3 className="text-sm font-medium text-gray-500">Credentials Emailed</h3>
          <p className="mt-1 text-3xl font-bold text-green-600">{typedEvent.credentials_emailed || 0}</p>
        </div>
        <div className="card p-6">
          <h3 className="text-sm font-medium text-gray-500">Failed</h3>
          <p className="mt-1 text-3xl font-bold text-red-600">{typedEvent.credentials_failed || 0}</p>
        </div>
      </div>

      {typeError && (
        <div className="mb-6 bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg text-sm" role="alert">
          {typeError}
        </div>
      )}

      <div className="card">
        <div className="p-6 border-b border-gray-200 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-gray-900">Participant Types</h2>
          <button
            onClick={() => { setShowAddType((v) => !v); setTypeError(null); }}
            className="btn-secondary text-sm"
          >
            {showAddType ? 'Cancel' : '+ Add Type'}
          </button>
        </div>

        {showAddType && (
          <div className="p-6 border-b border-gray-200 bg-gray-50">
            <h3 className="text-sm font-medium text-gray-900 mb-4">New Participant Type</h3>
            <div className="grid gap-4 md:grid-cols-2">
              <div>
                <label className="label">Name</label>
                <input
                  type="text"
                  value={newTypeName}
                  onChange={(e) => setNewTypeName(e.target.value)}
                  className="input-error"
                  placeholder="e.g. Volunteer, Judge, Sponsor"
                />
              </div>
              <div className="flex items-end gap-6">
                <label className="flex items-center gap-2 text-sm text-gray-700 cursor-pointer">
                  <input type="checkbox" checked={newBadge} onChange={(e) => setNewBadge(e.target.checked)} className="h-4 w-4 text-primary-600 rounded" />
                  Badge
                </label>
                <label className="flex items-center gap-2 text-sm text-gray-700 cursor-pointer">
                  <input type="checkbox" checked={newCert} onChange={(e) => setNewCert(e.target.checked)} className="h-4 w-4 text-primary-600 rounded" />
                  Certificate
                </label>
              </div>
            </div>
            <div className="mt-4 flex gap-2">
              <button onClick={handleCreateType} disabled={createPending} className="btn-primary text-sm">
                {createPending ? 'Adding...' : 'Add Type'}
              </button>
            </div>
          </div>
        )}

        <div className="divide-y divide-gray-200">
          {typedEvent.participant_types?.map((pt: ParticipantTypeWithCounts) => (
            <div key={pt.id} className="p-6 flex flex-col md:flex-row md:items-center md:justify-between gap-4">
              <div className="flex items-center gap-4">
                <div className="w-3 h-3 rounded-full" style={{ backgroundColor: typedEvent.accent_color }}></div>
                <div>
                  <h3 className="font-medium text-gray-900">{pt.name}</h3>
                  <p className="text-sm text-gray-500">{pt.participant_count} participants</p>
                </div>
              </div>
              <div className="flex flex-col sm:flex-row items-start sm:items-center gap-4 flex-wrap">
                <div className="flex items-center gap-2">
                  <span className="text-sm text-gray-600">Badge</span>
                  <label className="relative inline-flex items-center cursor-pointer">
                    {badgeToggle(pt)}
                    <div className="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary-300 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-primary-600"></div>
                  </label>
                  {pt.badge_template ? (
                    <span className="text-green-600 text-sm">✓ Template</span>
                  ) : (
                    <span className="text-gray-400 text-sm">No template</span>
                  )}
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-sm text-gray-600">Certificate</span>
                  <label className="relative inline-flex items-center cursor-pointer">
                    {certToggle(pt)}
                    <div className="w-11 h-6 bg-gray-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-primary-300 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-primary-600"></div>
                  </label>
                  {pt.certificate_template ? (
                    <span className="text-green-600 text-sm">✓ Template</span>
                  ) : (
                    <span className="text-gray-400 text-sm">No template</span>
                  )}
                </div>
                <Link to={`/events/${id}/issue`} className="btn-primary text-sm">
                  Issue & Send
                </Link>
                <button
                  onClick={() => {
                    if (confirm(`Delete participant type "${pt.name}"?`)) {
                      deleteType(pt.id);
                    }
                  }}
                  className="text-red-600 hover:text-red-900 text-sm font-medium"
                >
                  Delete
                </button>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}