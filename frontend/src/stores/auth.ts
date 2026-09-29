import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { User, AuthResponse } from '../types';
import { setAuthToken, getAuthToken } from '../api/client';

interface AuthState {
  user: User | null;
  token: string | null;
  isAuthenticated: boolean;
  login: (data: AuthResponse) => void;
  logout: () => void;
  initialize: () => void;
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set) => ({
      user: null,
      token: null,
      isAuthenticated: false,
      login: (data: AuthResponse) => {
        setAuthToken(data.token);
        set({ user: data.user, token: data.token, isAuthenticated: true });
      },
      logout: () => {
        setAuthToken(null);
        set({ user: null, token: null, isAuthenticated: false });
      },
      initialize: () => {
        const token = getAuthToken();
        if (token) {
          set({ token, isAuthenticated: true });
        }
      },
    }),
    {
      name: 'auth-storage',
      partialize: (state) => ({ token: state.token, user: state.user, isAuthenticated: state.isAuthenticated }),
    }
  )
);