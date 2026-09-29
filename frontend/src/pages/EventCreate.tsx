import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { createEvent, Event } from '../api/client';
import { useMutation, useQueryClient } from '@tanstack/react-query';

const eventSchema = z.object({
  name: z.string().min(1, 'Event name is required'),
  event_date: z.string().optional(),
  org_name: z.string().optional(),
  accent_color: z.string().default('#4F46E5'),
});

type EventForm = z.infer<typeof eventSchema>;

export default function EventCreate() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [color, setColor] = useState('#4F46E5');

  const { mutate: create, isPending, error } = useMutation({
    mutationFn: createEvent,
    onSuccess: (event: Event) => {
      queryClient.invalidateQueries({ queryKey: ['events'] });
      navigate(`/events/${event.id}`);
    },
    onError: (err) => console.error(err),
  });

  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm<EventForm>({
    resolver: zodResolver(eventSchema),
    defaultValues: {
      accent_color: '#4F46E5',
    },
  });

  const onSubmit = (data: EventForm) => {
    create({ ...data, accent_color: color });
  };

  return (
    <div className="max-w-2xl">
      <div className="mb-8">
        <h1 className="text-3xl font-bold text-gray-900">Create New Event</h1>
        <p className="mt-2 text-gray-600">Set up your event details and start issuing credentials.</p>
      </div>

      <form className="card p-6 space-y-6" onSubmit={handleSubmit(onSubmit)}>
        {error && (
          <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg text-sm" role="alert">
            {error.message}
          </div>
        )}

        <div>
          <label htmlFor="name" className="label">Event name</label>
          <input
            id="name"
            type="text"
            {...register('name')}
            className={`input-error ${errors.name ? 'border-red-500' : ''}`}
            placeholder="TechConf 2025"
          />
          {errors.name && <p className="error-text">{errors.name.message}</p>}
        </div>

        <div>
          <label htmlFor="event_date" className="label">Event date (optional)</label>
          <input
            id="event_date"
            type="date"
            {...register('event_date')}
            className="input-error"
          />
        </div>

        <div>
          <label htmlFor="org_name" className="label">Organization name (optional)</label>
          <input
            id="org_name"
            type="text"
            {...register('org_name')}
            className="input-error"
            placeholder="TechConf Org"
          />
        </div>

        <div>
          <label className="label">Accent color</label>
          <div className="flex items-center gap-4">
            <input
              type="color"
              value={color}
              onChange={(e) => setColor(e.target.value)}
              className="w-12 h-12 rounded-lg border border-gray-300 cursor-pointer"
            />
            <input
              type="text"
              {...register('accent_color')}
              className="input-error flex-1 font-mono text-sm"
              placeholder="#4F46E5"
            />
          </div>
        </div>

        <div className="flex justify-end gap-4 pt-4 border-t border-gray-200">
          <button type="button" onClick={() => navigate('/events')} className="btn-secondary">
            Cancel
          </button>
          <button type="submit" disabled={isPending} className="btn-primary">
            {isPending ? 'Creating...' : 'Create Event'}
          </button>
        </div>
      </form>
    </div>
  );
}