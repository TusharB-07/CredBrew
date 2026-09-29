import { useState } from 'react';
import { Link } from 'react-router-dom';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useLogin } from '../hooks/useAuth';

const loginSchema = z.object({
  email: z.string().email('Invalid email address'),
  password: z.string().min(1, 'Password is required'),
});

type LoginForm = z.infer<typeof loginSchema>;

export default function Login() {
  const { mutate: login, isPending, error } = useLogin();
  const [formError, setFormError] = useState<string | null>(null);

  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm<LoginForm>({
    resolver: zodResolver(loginSchema),
  });

  const onSubmit = (data: LoginForm) => {
    setFormError(null);
    login(data, {
      onError: (err) => setFormError(err.message),
    });
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-50 px-4">
      <div className="max-w-md w-full space-y-8">
        <div>
          <Link to="/events" className="text-3xl font-bold text-primary-600">CredBrew</Link>
          <h2 className="mt-6 text-center text-3xl font-extrabold text-gray-900">Sign in to your account</h2>
          <p className="mt-2 text-center text-sm text-gray-600">
            Or{' '}
            <Link to="/register" className="font-medium text-primary-600 hover:text-primary-500">
              create a new account
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
              <label htmlFor="email" className="label">Email address</label>
              <input
                id="email"
                type="email"
                autoComplete="email"
                {...register('email')}
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
                autoComplete="current-password"
                {...register('password')}
                className={`input-error ${errors.password ? 'border-red-500' : ''}`}
                placeholder="••••••••"
              />
              {errors.password && <p className="error-text">{errors.password.message}</p>}
            </div>
          </div>
          <div>
            <button
              type="submit"
              disabled={isPending}
              className="btn-primary w-full py-3"
            >
              {isPending ? 'Signing in...' : 'Sign in'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}