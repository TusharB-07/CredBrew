import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { listParticipants, createParticipant, importParticipants, deleteParticipant, ParticipantWithCredentialStatus, ImportResult } from '../api/client';
import { getEvent, ParticipantType } from '../api/client';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';

const participantSchema = z.object({
  name: z.string().min(1, 'Name is required'),
  email: z.string().email('Invalid email address'),
  participant_type_id: z.string().uuid('Please select a participant type'),
});

type ParticipantForm = z.infer<typeof participantSchema>;

export default function Participants() {
  const { id } = useParams<{ id: string }>();
  const queryClient = useQueryClient();
  const [page, setPage] = useState(1);
  const [typeFilter, setTypeFilter] = useState<string>('');
  const [showImport, setShowImport] = useState(false);
  const [importResult, setImportResult] = useState<ImportResult | null>(null);

  const { data: event } = useQuery({
    queryKey: ['event', id],
    queryFn: () => getEvent(id!),
    enabled: !!id,
  });

  const { data: types = [] } = useQuery({
    queryKey: ['participant-types', id],
    queryFn: async () => {
      const eventData = await getEvent(id!);
      return (eventData as any).participant_types || [];
    },
    enabled: !!id,
  });

  const { data: participantsData, isLoading } = useQuery({
    queryKey: ['participants', id, page, typeFilter],
    queryFn: () => listParticipants(id!, { page, per_page: 50, type_id: typeFilter || undefined }),
    enabled: !!id,
  });

  const { mutate: create, isPending: createPending } = useMutation({
    mutationFn: (data: ParticipantForm) => createParticipant(id!, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['participants', id] });
    },
  });

  const { mutate: importMutate, isPending: importPending } = useMutation({
    mutationFn: (file: File) => importParticipants(id!, file),
    onSuccess: (result) => {
      setImportResult(result);
      queryClient.invalidateQueries({ queryKey: ['participants', id] });
    },
  });

  const { mutate: deleteMutate } = useMutation({
    mutationFn: deleteParticipant,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['participants', id] });
    },
  });

  const {
    register,
    handleSubmit,
    reset,
    formState: { errors },
  } = useForm<ParticipantForm>({
    resolver: zodResolver(participantSchema),
  });

  const onSubmit = (data: ParticipantForm) => {
    create(data);
    reset({ participant_type_id: '' });
  };

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) {
      importMutate(file);
    }
  };

  return (
    <div>
      <div className="mb-8">
        <Link to={`/events/${id}`} className="text-sm text-gray-500 hover:text-gray-700 mb-4 inline-block">
          ← Back to Event
        </Link>
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-3xl font-bold text-gray-900">Participants</h1>
            <p className="mt-1 text-gray-500">{event?.name}</p>
          </div>
          <div className="flex gap-2">
            <button onClick={() => setShowImport(true)} className="btn-secondary">
              Import CSV
            </button>
            <Link to={`/events/${id}/issue`} className="btn-primary">Issue & Send</Link>
          </div>
        </div>
      </div>

      <div className="grid gap-6 lg:grid-cols-3">
        <div className="lg:col-span-2 card">
          <div className="p-6 border-b border-gray-200 flex items-center justify-between">
            <h2 className="text-lg font-semibold text-gray-900">Participant List</h2>
            <select
              value={typeFilter}
              onChange={(e) => { setTypeFilter(e.target.value); setPage(1); }}
              className="input-error w-auto"
            >
              <option value="">All Types</option>
              {types.map((t: ParticipantType) => (
                <option key={t.id} value={t.id}>{t.name}</option>
              ))}
            </select>
          </div>

          {isLoading ? (
            <div className="p-6 text-center">
              <div className="animate-spin rounded-full h-8 w-8 border-4 border-primary-600 border-t-transparent mx-auto"></div>
            </div>
          ) : (
            <>
              <div className="overflow-x-auto">
                <table className="w-full">
                  <thead>
                    <tr className="text-left text-sm text-gray-500 border-b border-gray-200">
                      <th className="pb-3 px-6 font-medium">Name</th>
                      <th className="pb-3 px-6 font-medium">Email</th>
                      <th className="pb-3 px-6 font-medium">Type</th>
                      <th className="pb-3 px-6 font-medium">Badge</th>
                      <th className="pb-3 px-6 font-medium">Certificate</th>
                      <th className="pb-3 px-6 font-medium">Actions</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-100">
                    {participantsData?.participants.map((p: ParticipantWithCredentialStatus) => (
                      <tr key={p.id} className="hover:bg-gray-50">
                        <td className="py-4 px-6">{p.name}</td>
                        <td className="py-4 px-6">{p.email}</td>
                        <td className="py-4 px-6">
                          <span className="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium bg-primary-100 text-primary-800">
                            {types.find((t: ParticipantType) => t.id === p.participant_type_id)?.name}
                          </span>
                        </td>
                        <td className="py-4 px-6">
                          <span className={p.badge_status === 'emailed' ? 'text-green-600' : p.badge_status === 'failed' ? 'text-red-600' : 'text-gray-400'}>
                            {p.badge_status || '—'}
                          </span>
                        </td>
                        <td className="py-4 px-6">
                          <span className={p.certificate_status === 'emailed' ? 'text-green-600' : p.certificate_status === 'failed' ? 'text-red-600' : 'text-gray-400'}>
                            {p.certificate_status || '—'}
                          </span>
                        </td>
                        <td className="py-4 px-6">
                          <button
                            onClick={() => {
                              if (confirm('Delete this participant?')) {
                                deleteMutate(p.id);
                              }
                            }}
                            className="text-red-600 hover:text-red-900 text-sm font-medium"
                          >
                            Delete
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              {participantsData?.participants.length === 0 && (
                <div className="p-12 text-center text-gray-500">No participants found</div>
              )}

              <div className="p-6 border-t border-gray-200 flex items-center justify-between">
                <p className="text-sm text-gray-500">
                  Showing {(page - 1) * 50 + 1} to {Math.min(page * 50, participantsData?.total || 0)} of {participantsData?.total || 0}
                </p>
                <div className="flex gap-2">
                  <button
                    onClick={() => setPage(p => Math.max(1, p - 1))}
                    disabled={page === 1}
                    className="btn-secondary text-sm px-3 py-1"
                  >
                    Previous
                  </button>
                  <button
                    onClick={() => setPage(p => p + 1)}
                    disabled={page * 50 >= (participantsData?.total || 0)}
                    className="btn-secondary text-sm px-3 py-1"
                  >
                    Next
                  </button>
                </div>
              </div>
            </>
          )}
        </div>

        <div className="card p-6">
          <h2 className="text-lg font-semibold text-gray-900 mb-4">Add Participant</h2>
          <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
            <div>
              <label htmlFor="name" className="label">Name</label>
              <input
                id="name"
                {...register('name')}
                className={`input-error ${errors.name ? 'border-red-500' : ''}`}
                placeholder="John Doe"
              />
              {errors.name && <p className="error-text">{errors.name.message}</p>}
            </div>
            <div>
              <label htmlFor="email" className="label">Email</label>
              <input
                id="email"
                type="email"
                {...register('email')}
                className={`input-error ${errors.email ? 'border-red-500' : ''}`}
                placeholder="john@example.com"
              />
              {errors.email && <p className="error-text">{errors.email.message}</p>}
            </div>
            <div>
              <label htmlFor="participant_type_id" className="label">Type</label>
              <select
                id="participant_type_id"
                {...register('participant_type_id')}
                className={`input-error ${errors.participant_type_id ? 'border-red-500' : ''}`}
              >
                <option value="">Select type</option>
                {types.map((t: ParticipantType) => (
                  <option key={t.id} value={t.id}>{t.name}</option>
                ))}
              </select>
              {errors.participant_type_id && <p className="error-text">{errors.participant_type_id.message}</p>}
            </div>
            <button type="submit" disabled={createPending} className="btn-primary w-full">
              {createPending ? 'Adding...' : 'Add Participant'}
            </button>
          </form>
        </div>
      </div>

      {showImport && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center p-4 z-50">
          <div className="bg-white rounded-xl shadow-xl max-w-md w-full p-6">
            <h2 className="text-xl font-semibold text-gray-900 mb-4">Import Participants from CSV</h2>
            <p className="text-sm text-gray-500 mb-4">
              CSV columns: name, email, type (participant type name). Max 5000 rows.
            </p>
            <input
              type="file"
              accept=".csv"
              onChange={handleFileChange}
              disabled={importPending}
              className="input-error mb-4"
            />
            {importPending && <p className="text-sm text-gray-500 mb-4">Importing...</p>}
            {importResult && (
              <div className="mb-4 p-4 bg-gray-50 rounded-lg">
                <p className="font-medium">Imported: {importResult.imported}</p>
                {importResult.errors.length > 0 && (
                  <div className="mt-2 max-h-40 overflow-y-auto">
                    <p className="text-sm font-medium text-red-600 mb-1">Errors ({importResult.errors.length}):</p>
                    <ul className="text-sm text-red-500 space-y-1">
                      {importResult.errors.slice(0, 10).map((err, i) => (
                        <li key={i}>Row {err.row} ({err.field}): {err.message}</li>
                      ))}
                      {importResult.errors.length > 10 && (
                        <li>... and {importResult.errors.length - 10} more</li>
                      )}
                    </ul>
                  </div>
                )}
              </div>
            )}
            <div className="flex justify-end gap-2">
              <button onClick={() => { setShowImport(false); setImportResult(null); }} className="btn-secondary">Close</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}