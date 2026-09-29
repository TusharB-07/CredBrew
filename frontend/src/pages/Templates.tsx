import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { createTemplate, previewTemplate, getEvent, ParticipantType } from '../api/client';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';

const templateSchema = z.object({
  participant_type_id: z.string().uuid('Please select a participant type'),
  kind: z.enum(['badge', 'certificate']),
  name_zone: z.object({
    x_pct: z.number().default(10),
    y_pct: z.number().default(70),
    w_pct: z.number().default(80),
    font_size_pct: z.number().default(6),
    color: z.string().default('#111111'),
    align: z.string().default('center'),
  }),
  qr_zone: z.object({
    x_pct: z.number().default(85),
    y_pct: z.number().default(5),
    size_pct: z.number().default(10),
  }),
});

type TemplateForm = z.infer<typeof templateSchema>;

const defaultNameZone = {
  x_pct: 10,
  y_pct: 70,
  w_pct: 80,
  font_size_pct: 6,
  color: '#111111',
  align: 'center',
};

const defaultQrZone = {
  x_pct: 85,
  y_pct: 5,
  size_pct: 10,
};

export default function Templates() {
  const { id } = useParams<{ id: string }>();
  const queryClient = useQueryClient();
  const [preview, setPreview] = useState<{ blob: Blob; kind: 'badge' | 'certificate' } | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);

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

  const { mutate: create, isPending } = useMutation({
    mutationFn: (data: { form: TemplateForm; file: File }) => createTemplate({
      participant_type_id: data.form.participant_type_id,
      kind: data.form.kind,
      file: data.file,
      name_zone: data.form.name_zone,
      qr_zone: data.form.qr_zone,
    }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['templates', id] });
      setPreview(null);
    },
  });

  const {
    register,
    handleSubmit,
    watch,
    formState: { errors },
  } = useForm<TemplateForm>({
    resolver: zodResolver(templateSchema),
    defaultValues: {
      kind: 'badge',
      name_zone: defaultNameZone,
      qr_zone: defaultQrZone,
    },
  });

  const watchedKind = watch('kind');
  const watchedTypeId = watch('participant_type_id');

  const onSubmit = (data: TemplateForm) => {
    const fileInput = document.getElementById('template-file') as HTMLInputElement;
    if (fileInput.files?.[0]) {
      create({ form: data, file: fileInput.files[0] });
    }
  };

  const handlePreview = async () => {
    const fileInput = document.getElementById('template-file') as HTMLInputElement;
    if (!fileInput.files?.[0]) {
      setPreviewError('Please select an SVG file first');
      return;
    }
    setPreviewError(null);
    try {
      const blob = await previewTemplate(watchedTypeId || '');
      setPreview({ blob, kind: watchedKind });
    } catch (err) {
      setPreviewError(err instanceof Error ? err.message : 'Preview failed');
    }
  };

  return (
    <div>
      <div className="mb-8">
        <Link to={`/events/${id}`} className="text-sm text-gray-500 hover:text-gray-700 mb-4 inline-block">
          ← Back to Event
        </Link>
        <h1 className="text-3xl font-bold text-gray-900">Templates</h1>
        <p className="mt-1 text-gray-500">Upload SVG templates for badges and certificates</p>
      </div>

      <div className="grid gap-6 lg:grid-cols-2">
        <div className="card p-6">
          <h2 className="text-lg font-semibold text-gray-900 mb-6">Upload Template</h2>
          <form onSubmit={handleSubmit(onSubmit)} className="space-y-6">
            {previewError && (
              <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg text-sm" role="alert">
                {previewError}
              </div>
            )}

            <div>
              <label className="label">Participant Type</label>
              <select
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

            <div>
              <label className="label">Kind</label>
              <div className="flex gap-4">
                {['badge', 'certificate'].map((k) => (
                  <label key={k} className="flex items-center gap-2 cursor-pointer">
                    <input
                      type="radio"
                      value={k}
                      {...register('kind')}
                      className="text-primary-600 focus:ring-primary-500"
                    />
                    <span className="capitalize">{k}</span>
                  </label>
                ))}
              </div>
            </div>

            <div>
              <label className="label">SVG File (max 2MB)</label>
              <input
                id="template-file"
                type="file"
                accept="image/svg+xml, .svg"
                className="input-error"
              />
              <p className="mt-1 text-sm text-gray-500">Must contain {"{{NAME}}"} placeholder and an element with id="qr-zone"</p>
            </div>

            <div className="border-t border-gray-200 pt-6">
              <h3 className="text-medium font-medium text-gray-900 mb-4">Name Zone</h3>
              <div className="grid gap-4 md:grid-cols-2">
                <div>
                  <label className="label">X %</label>
                  <input type="number" step="0.1" {...register('name_zone.x_pct')} className="input-error" />
                </div>
                <div>
                  <label className="label">Y %</label>
                  <input type="number" step="0.1" {...register('name_zone.y_pct')} className="input-error" />
                </div>
                <div>
                  <label className="label">Width %</label>
                  <input type="number" step="0.1" {...register('name_zone.w_pct')} className="input-error" />
                </div>
                <div>
                  <label className="label">Font Size %</label>
                  <input type="number" step="0.1" {...register('name_zone.font_size_pct')} className="input-error" />
                </div>
                <div>
                  <label className="label">Color</label>
                  <input type="color" {...register('name_zone.color')} className="w-12 h-12 rounded-lg border border-gray-300 cursor-pointer" />
                </div>
                <div>
                  <label className="label">Align</label>
                  <select {...register('name_zone.align')} className="input-error">
                    <option value="center">Center</option>
                    <option value="left">Left</option>
                    <option value="right">Right</option>
                  </select>
                </div>
              </div>
            </div>

            <div className="border-t border-gray-200 pt-6">
              <h3 className="text-medium font-medium text-gray-900 mb-4">QR Zone</h3>
              <div className="grid gap-4 md:grid-cols-3">
                <div>
                  <label className="label">X %</label>
                  <input type="number" step="0.1" {...register('qr_zone.x_pct')} className="input-error" />
                </div>
                <div>
                  <label className="label">Y %</label>
                  <input type="number" step="0.1" {...register('qr_zone.y_pct')} className="input-error" />
                </div>
                <div>
                  <label className="label">Size %</label>
                  <input type="number" step="0.1" {...register('qr_zone.size_pct')} className="input-error" />
                </div>
              </div>
            </div>

            <div className="flex gap-2">
              <button type="button" onClick={handlePreview} className="btn-secondary">Preview</button>
              <button type="submit" disabled={isPending} className="btn-primary">{isPending ? 'Uploading...' : 'Upload Template'}</button>
            </div>
          </form>
        </div>

        <div className="card p-6">
          <h2 className="text-lg font-semibold text-gray-900 mb-4">Preview</h2>
          {preview && (
            <div className="space-y-4">
              <div className="bg-gray-100 rounded-lg p-4 min-h-[300px] flex items-center justify-center">
                {preview.kind === 'badge' ? (
                  <img src={URL.createObjectURL(preview.blob)} alt="Badge preview" className="max-w-full max-h-96" />
                ) : (
                  <iframe src={URL.createObjectURL(preview.blob)} className="w-full h-96 border-0" title="Certificate preview" />
                )}
              </div>
              <p className="text-sm text-gray-500 text-center">Preview shows "Jane Doe" with sample QR code</p>
            </div>
          )}
          {!preview && (
            <div className="bg-gray-50 rounded-lg p-12 text-center text-gray-500">
              <svg className="mx-auto h-12 w-12 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z" />
              </svg>
              <p className="mt-2">Upload an SVG and click Preview to see the rendered output</p>
            </div>
          )}
        </div>
      </div>

      <div className="mt-8 card">
        <div className="p-6 border-b border-gray-200">
          <h2 className="text-lg font-semibold text-gray-900">Existing Templates</h2>
        </div>
        <div className="divide-y divide-gray-200">
          {types.map((t: ParticipantType & { badge_template?: boolean; certificate_template?: boolean }) => (
            <>
              <div key={t.id + '-badge'} className="p-4 flex items-center justify-between">
                <div className="flex items-center gap-4">
                  <div className="w-3 h-3 rounded-full" style={{ backgroundColor: event?.accent_color }}></div>
                  <div>
                    <p className="font-medium">{t.name} - Badge</p>
                    <p className="text-sm text-gray-500">{t.badge_template ? 'Uploaded' : 'Not uploaded'}</p>
                  </div>
                </div>
                {t.badge_template && (
                  <Link to={`/events/${id}/templates`} className="btn-secondary text-sm">Re-upload</Link>
                )}
              </div>
              <div key={t.id + '-cert'} className="p-4 flex items-center justify-between">
                <div className="flex items-center gap-4">
                  <div className="w-3 h-3 rounded-full" style={{ backgroundColor: event?.accent_color }}></div>
                  <div>
                    <p className="font-medium">{t.name} - Certificate</p>
                    <p className="text-sm text-gray-500">{t.certificate_template ? 'Uploaded' : 'Not uploaded'}</p>
                  </div>
                </div>
                {t.certificate_template && (
                  <Link to={`/events/${id}/templates`} className="btn-secondary text-sm">Re-upload</Link>
                )}
              </div>
            </>
          ))}
        </div>
      </div>
    </div>
  );
}