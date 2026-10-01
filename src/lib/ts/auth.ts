import { get, writable } from 'svelte/store';

export type User = {
    id: string;
    username: string;
    email: string | null;
    is_admin: boolean;
    /** External user: only sees what was shared with them, cannot create content. */
    is_guest: boolean;
    avatar_url: string | null;
};

export const userStore = writable<User | null>(null);

export async function fetchUser() {
    try {
        const res = await fetch('/api/auth/me');
        if (res.ok) {
            const user = await res.json();
            userStore.set(user);
        } else {
            userStore.set(null);
        }
    } catch {
        userStore.set(null);
    }
}

/**
 * Sends the browser to the identity provider. With an active IdP session this
 * round-trips without any prompt (seamless SSO) and lands back on the current page.
 */
export function redirectToLogin(returnTo: string = window.location.pathname + window.location.search) {
    window.location.href = `/api/auth/oidc/login?return_to=${encodeURIComponent(returnTo)}`;
}

/**
 * Sends the browser through sign-in again when an API call reports that a signed-in
 * user's session ended (expired, revoked at the IdP, or renewed after profile
 * settings changed). With an active IdP session this returns silently to the page.
 */
export function watchSessionLoss() {
    const original = window.fetch.bind(window);
    let redirecting = false;
    window.fetch = async (input, init) => {
        const response = await original(input, init);
        const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
        const path = new URL(url, window.location.href).pathname;
        if (
            response.status === 401 &&
            !redirecting &&
            get(userStore) &&
            path.startsWith('/api/') &&
            !path.startsWith('/api/auth/logout')
        ) {
            redirecting = true;
            userStore.set(null);
            redirectToLogin();
        }
        return response;
    };
}

/** Ends the Trykst session, then the IdP session. */
export async function logout() {
    let logoutUrl: string | null = null;
    try {
        const res = await fetch('/api/auth/logout', { method: 'POST' });
        if (res.ok) {
            logoutUrl = (await res.json()).logout_url ?? null;
        }
    } catch {
        // Fall through: the local session cookie is gone either way.
    }
    userStore.set(null);
    window.location.href = logoutUrl ?? '/';
}
