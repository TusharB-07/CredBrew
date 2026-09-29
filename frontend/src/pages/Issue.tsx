import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { issueCredentials, getIssueStatus, retryCredential, getEvent, ParticipantType } from '../api/client';

export default function Issue() {
  const { id } = useParams<{ id: string }>();
  const queryClient = useQueryClient();
  const [selectedTypeId, setSelectedTypeId] = useState<string>('');
  const [selectedKind, setSelectedKind] = useState<'badge' | 'certificate' | 'both'>('both');
  const [sendEmail, setSendEmail] = useState(true);
  const [polling, setPolling] = useState(false);

  const { data: types = [] } = useQuery({
    queryKey: ['participant-types', id],
    queryFn: async () => {
      const eventData = await getEvent(id!);
      return (eventData as any).participant_types || [];
    },
    enabled: !!id,
  });

  const { data: status, refetch } = useQuery({
    queryKey: ['issue-status', id, selectedTypeId, selectedKind],
    queryFn: () => getIssueStatus(id!, { type_id: selectedTypeId || undefined, kind: selectedKind === 'both' ? undefined : selectedKind }),
    enabled: !!id && !!selectedTypeId,
    refetchInterval: polling ? 2000 : false,
  });

  const { mutate: issue, isPending: issuePending } = useMutation({
    mutationFn: (data: { participant_type_id: string; kind: 'badge' | 'certificate' | 'both'; send_email: boolean }) =>
      issueCredentials(id!, data),
    onSuccess: () => {
      setPolling(true);
      setTimeout(() => setPolling(false), 30000);
    },
  });

  const { mutate: retry } = useMutation({
    mutationFn: retryCredential,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['issue-status', id] });
    },
  });

  const selectedType = types.find(
    (t: ParticipantType & { participant_count: number; badge_template?: boolean; certificate_template?: boolean }) =>
      t.id === selectedTypeId,
  );
  const badgeTemplate = selectedType?.badge_template;
  const certTemplate = selectedType?.certificate_template;

  const canIssue = selectedTypeId && (
    (selectedKind !== 'certificate' && badgeTemplate) ||
    (selectedKind !== 'badge' && certTemplate)
  );

  const handleIssue = () => {
    if (confirm(`Issue ${selectedKind === 'both' ? 'badges and certificates' : selectedKind} to ${selectedType?.participant_count || 0} participants and send emails?`)) {
      issue({ participant_type_id: selectedTypeId, kind: selectedKind, send_email: sendEmail });
    }
  };

  const handleSendTest = async () => {
    alert('Test email feature would send to your account email. Implement by calling issue with send_email=true for a single test participant.');
  };

  return (
    <div>
      <div className="mb-8">
        <Link to={`/events/${id}`} className="text-sm text-gray-500 hover:text-gray-700 mb-4 inline-block">
          ← Back to Event
        </Link>
        <h1 className="text-3xl font-bold text-gray-900">Issue & Send</h1>
        <p className="mt-1 text-gray-500">Send credentials to participants</p>
      </div>

      <div className="grid gap-6 lg:grid-cols-3">
        <div className="lg:col-span-2 space-y-6">
          <div className="card p-6">
            <h2 className="text-lg font-semibold text-gray-900 mb-4">Pre-send Checklist</h2>
            <div className="space-y-3">
              <label className="flex items-center gap-3 cursor-pointer">
                <input type="checkbox" checked={!!selectedTypeId} disabled className="h-4 w-4 text-primary-600 rounded" />
                <span className={selectedTypeId ? 'text-gray-900' : 'text-gray-400'}>
                  Participant type selected: {selectedType?.name || 'None'}
                </span>
              </label>
              <label className="flex items-center gap-3 cursor-pointer">
                <input type="checkbox" checked={selectedKind !== 'certificate' && !!badgeTemplate} disabled className="h-4 w-4 text-primary-600 rounded" />
                <span className={(selectedKind !== 'certificate' && badgeTemplate) ? 'text-gray-900' : 'text-gray-400'}>
                  Badge template: {badgeTemplate ? '✓ Ready' : '✗ Missing'}
                </span>
              </label>
              <label className="flex items-center gap-3 cursor-pointer">
                <input type="checkbox" checked={selectedKind !== 'badge' && !!certTemplate} disabled className="h-4 w-4 text-primary-600 rounded" />
                <span className={(selectedKind !== 'badge' && certTemplate) ? 'text-gray-900' : 'text-gray-400'}>
                  Certificate template: {certTemplate ? '✓ Ready' : '✗ Missing'}
                </span>
              </label>
              <label className="flex items-center gap-3 cursor-pointer">
                <input type="checkbox" checked={selectedType && (selectedType.participant_count || 0) > 0} disabled className="h-4 w-4 text-primary-600 rounded" />
                <span className={selectedType && (selectedType.participant_count || 0) > 0 ? 'text-gray-900' : 'text-gray-400'}>
                  Participants: {selectedType?.participant_count || 0}
                </span>
              </label>
            </div>
            <button
              onClick={handleSendTest}
              className="mt-4 btn-secondary text-sm"
              disabled={!selectedTypeId}
            >
              Send test email to self
            </button>
          </div>

          <div className="card p-6">
            <h2 className="text-lg font-semibold text-gray-900 mb-4">Issue Configuration</h2>
            <div className="space-y-4">
              <div>
                <label className="label">Participant Type</label>
                <select
                  value={selectedTypeId}
                  onChange={(e) => setSelectedTypeId(e.target.value)}
                  className="input-error"
                >
                  <option value="">Select type</option>
                  {types.map((t: ParticipantType & { participant_count: number }) => (
                    <option key={t.id} value={t.id}>{t.name} ({t.participant_count} participants)</option>
                  ))}
                </select>
              </div>

              <div>
                <label className="label">What to Issue</label>
                <div className="flex gap-4">
                  {['badge', 'certificate', 'both'].map((k) => (
                    <label key={k} className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="radio"
                        value={k}
                        checked={selectedKind === k}
                        onChange={() => setSelectedKind(k as 'badge' | 'certificate' | 'both')}
                        className="text-primary-600 focus:ring-primary-500"
                      />
                      <span className="capitalize">{k === 'both' ? 'Badge & Certificate' : k}</span>
                    </label>
                  ))}
                </div>
              </div>

              <div>
                <label className="flex items-center gap-3 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={sendEmail}
                    onChange={(e) => setSendEmail(e.target.checked)}
                    className="h-4 w-4 text-primary-600 rounded"
                  />
                  <span>Send emails to participants</span>
                </label>
              </div>

              <button
                onClick={handleIssue}
                disabled={!canIssue || issuePending}
                className="btn-primary w-full py-3 text-lg mt-4"
              >
                {issuePending ? 'Issuing...' : 'Issue & Send'}
              </button>
            </div>
          </div>

          {polling && status && (
            <div className="card p-6">
              <h2 className="text-lg font-semibold text-gray-900 mb-4">Progress</h2>
              <div className="space-y-4">
                <div className="flex items-center gap-4 text-sm">
                  <span className="w-24 text-gray-500">Pending</span>
                  <div className="flex-1 h-2 bg-gray-200 rounded-full overflow-hidden">
                    <div
                      className="h-full bg-yellow-500 transition-all"
                      style={{ width: `${((status.pending / status.total) * 100) || 0}%` }}
                    ></div>
                  </div>
                  <span className="w-12 text-right">{status.pending}</span>
                </div>
                <div className="flex items-center gap-4 text-sm">
                  <span className="w-24 text-gray-500">Rendering</span>
                  <div className="flex-1 h-2 bg-gray-200 rounded-full overflow-hidden">
                    <div
                      className="h-full bg-blue-500 transition-all"
                      style={{ width: `${((status.rendered / status.total) * 100) || 0}%` }}
                    ></div>
                  </div>
                  <span className="w-12 text-right">{status.rendered}</span>
                </div>
                <div className="flex items-center gap-4 text-sm">
                  <span className="w-24 text-gray-500">Emailed</span>
                  <div className="flex-1 h-2 bg-gray-200 rounded-full overflow-hidden">
                    <div
                      className="h-full bg-green-500 transition-all"
                      style={{ width: `${((status.emailed / status.total) * 100) || 0}%` }}
                    ></div>
                  </div>
                  <span className="w-12 text-right">{status.emailed}</span>
                </div>
                <div className="flex items-center gap-4 text-sm">
                  <span className="w-24 text-gray-500">Failed</span>
                  <div className="flex-1 h-2 bg-gray-200 rounded-full overflow-hidden">
                    <div
                      className="h-full bg-red-500 transition-all"
                      style={{ width: `${((status.failed / status.total) * 100) || 0}%` }}
                    ></div>
                  </div>
                  <span className="w-12 text-right">{status.failed}</span>
                </div>
                <div className="pt-4 border-t border-gray-200">
                  <button onClick={() => refetch()} className="btn-secondary text-sm">Refresh</button>
                </div>
              </div>
            </div>
          )}

          {status && status.recent_failures.length > 0 && (
            <div className="card p-6">
              <h2 className="text-lg font-semibold text-gray-900 mb-4">Failed Credentials</h2>
              <div className="overflow-x-auto">
                <table className="w-full">
                  <thead>
                    <tr className="text-left text-sm text-gray-500 border-b border-gray-200">
                      <th className="pb-3 px-6 font-medium">Participant</th>
                      <th className="pb-3 px-6 font-medium">Error</th>
                      <th className="pb-3 px-6 font-medium">Attempts</th>
                      <th className="pb-3 px-6 font-medium">Actions</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-100">
                    {status.recent_failures.map((f: any) => (
                      <tr key={f.credential_id} className="hover:bg-gray-50">
                        <td className="py-4 px-6">
                          <p className="font-medium">{f.participant_name}</p>
                          <p className="text-sm text-gray-500">{f.participant_email}</p>
                        </td>
                        <td className="py-4 px-6 text-sm text-red-600 max-w-xs truncate" title={f.error}>
                          {f.error}
                        </td>
                        <td className="py-4 px-6 text-sm">{f.attempts}</td>
                        <td className="py-4 px-6">
                          <button
                            onClick={() => retry(f.credential_id)}
                            className="btn-secondary text-sm"
                          >
                            Retry
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}
        </div>

        <div className="card p-6 sticky top-24">
          <h2 className="text-lg font-semibold text-gray-900 mb-4">Live Status</h2>
          {status ? (
            <div className="space-y-4">
              <div className="bg-gray-50 rounded-lg p-4">
                <p className="text-sm text-gray-500">Total</p>
                <p className="text-3xl font-bold text-gray-900">{status.total}</p>
              </div>
              <div className="grid grid-cols-2 gap-4">
                <div className="bg-green-50 rounded-lg p-4">
                  <p className="text-sm text-green-700">Emailed</p>
                  <p className="text-2xl font-bold text-green-600">{status.emailed}</p>
                </div>
                <div className="bg-red-50 rounded-lg p-4">
                  <p className="text-sm text-red-700">Failed</p>
                  <p className="text-2xl font-bold text-red-600">{status.failed}</p>
                </div>
                <div className="bg-yellow-50 rounded-lg p-4">
                  <p className="text-sm text-yellow-700">Pending</p>
                  <p className="text-2xl font-bold text-yellow-600">{status.pending}</p>
                </div>
                <div className="bg-blue-50 rounded-lg p-4">
                  <p className="text-sm text-blue-700">Rendering</p>
                  <p className="text-2xl font-bold text-blue-600">{status.rendered}</p>
                </div>
              </div>
              {polling && (
                <div className="text-center text-sm text-gray-500">
                  <span className="animate-pulse inline-block w-2 h-2 bg-primary-600 rounded-full mr-1"></span>
                  Auto-refreshing every 2s...
                </div>
              )}
            </div>
          ) : (
            <p className="text-gray-500 text-center py-8">Select a participant type to see status</p>
          )}
        </div>
      </div>
    </div>
  );
}