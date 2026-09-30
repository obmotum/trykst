import { writable } from 'svelte/store';

export type User = {
    id: string;
    username: string;
    email: string | null;
    is_admin: boolean;
    /** External user: only sees what was shared with them, cannot create content. */
    is_guest: boolean;
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

/** Ends the TypstDrive session, then the IdP session. */
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
