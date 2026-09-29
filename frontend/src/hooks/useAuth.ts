import { useAuthStore } from '../stores/auth';
import { useNavigate, useLocation } from 'react-router-dom';
import { useEffect } from 'react';
import { login, register } from '../api/client';
import { useMutation } from '@tanstack/react-query';

export function useLogin() {
  const navigate = useNavigate();
  const { login: setAuth } = useAuthStore();

  return useMutation({
    mutationFn: ({ email, password }: { email: string; password: string }) => login(email, password),
    onSuccess: (data) => {
      setAuth(data);
      navigate('/events');
    },
  });
}

export function useRegister() {
  const navigate = useNavigate();
  const { login: setAuth } = useAuthStore();

  return useMutation({
    mutationFn: ({ name, email, password }: { name: string; email: string; password: string }) => register(name, email, password),
    onSuccess: (data) => {
      setAuth(data);
      navigate('/events');
    },
  });
}

export function useAuth() {
  const { user, token, isAuthenticated, logout, initialize } = useAuthStore();
  const navigate = useNavigate();
  const location = useLocation();

  useEffect(() => {
    initialize();
  }, [initialize]);

  useEffect(() => {
    if (!isAuthenticated && location.pathname !== '/login' && location.pathname !== '/register') {
      navigate('/login');
    }
    if (isAuthenticated && (location.pathname === '/login' || location.pathname === '/register')) {
      navigate('/events');
    }
  }, [isAuthenticated, location.pathname, navigate]);

  return { user, token, isAuthenticated, logout };
}

export function useRequireAuth() {
  const { isAuthenticated, user } = useAuth();
  return { isAuthenticated, user };
}