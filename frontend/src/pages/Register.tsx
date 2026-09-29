import { useState } from 'react';
import { Link } from 'react-router-dom';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useRegister } from '../hooks/useAuth';

const registerSchema = z.object({
  name: z.string().min(2, 'Name must be at least 2 characters'),
  email: z.string().email('Invalid email address'),
  password: z.string().min(8, 'Password must be at least 8 characters'),
  confirmPassword: z.string(),
}).refine((data) => data.password === data.confirmPassword, {
  message: 'Passwords do not match',
  path: ['confirmPassword'],
});

type RegisterForm = z.infer<typeof registerSchema>;

export default function Register() {
  const { mutate: register, isPending, error } = useRegister();
  const [formError, setFormError] = useState<string | null>(null);

  const {
    register: registerField,
    handleSubmit,
    formState: { errors },
  } = useForm<RegisterForm>({
    resolver: zodResolver(registerSchema),
  });

  const onSubmit = (data: RegisterForm) => {
    setFormError(null);
    register(
      { name: data.name, email: data.email, password: data.password },
      {
        onError: (err) => setFormError(err.message),
      }
    );
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-50 px-4">
      <div className="max-w-md w-full space-y-8">
        <div>
          <Link to="/events" className="text-3xl font-bold text-primary-600">CredBrew</Link>
          <h2 className="mt-6 text-center text-3xl font-extrabold text-gray-900">Create your account</h2>
          <p className="mt-2 text-center text-sm text-gray-600">
            Already have an account?{' '}
            <Link to="/login" className="font-medium text-primary-600 hover:text-primary-500">
              Sign in
            </Link>
          </p>
        </div>
        <form className="mt-8 space-y-6" onSubmit={handleSubmit(onSubmit)}>
          {formError && (
            <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg text-sm" role="alert">
              {formError}
            </div>
          )}
          {error && (
            <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg text-sm" role="alert">
              {error.message}
            </div>
          )}
          <div className="space-y-4">
            <div>
              <label htmlFor="name" className="label">Full name</label>
              <input
                id="name"
                type="text"
                autoComplete="name"
                {...registerField('name')}
                className={`input-error ${errors.name ? 'border-red-500' : ''}`}
                placeholder="John Doe"
              />
              {errors.name && <p className="error-text">{errors.name.message}</p>}
            </div>
            <div>
              <label htmlFor="email" className="label">Email address</label>
              <input
                id="email"
                type="email"
                autoComplete="email"
                {...registerField('email')}
                className={`input-error ${errors.email ? 'border-red-500' : ''}`}
                placeholder="you@example.com"
              />
              {errors.email && <p className="error-text">{errors.email.message}</p>}
            </div>
            <div>
              <label htmlFor="password" className="label">Password</label>
              <input
                id="password"
                type="password"
                autoComplete="new-password"
                {...registerField('password')}
                className={`input-error ${errors.password ? 'border-red-500' : ''}`}
                placeholder="••••••••"
              />
              {errors.password && <p className="error-text">{errors.password.message}</p>}
            </div>
            <div>
              <label htmlFor="confirmPassword" className="label">Confirm password</label>
              <input
                id="confirmPassword"
                type="password"
                autoComplete="new-password"
                {...registerField('confirmPassword')}
                className={`input-error ${errors.confirmPassword ? 'border-red-500' : ''}`}
                placeholder="••••••••"
              />
              {errors.confirmPassword && <p className="error-text">{errors.confirmPassword.message}</p>}
            </div>
          </div>
          <div>
            <button
              type="submit"
              disabled={isPending}
              className="btn-primary w-full py-3"
            >
              {isPending ? 'Creating account...' : 'Create account'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}