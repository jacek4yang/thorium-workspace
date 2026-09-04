import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AccountCard from "./AccountCard";
import { api } from "../lib/api";
import { initI18n } from "../i18n";
import type { Account } from "../lib/types";

// vitest runs with globals:false, so cleanup must be explicit.
afterEach(cleanup);

vi.mock("./../lib/api", () => ({
  api: {
    passwordReveal: vi.fn(),
    passwordCopy: vi.fn(),
    passwordSet: vi.fn(),
    passwordDelete: vi.fn(),
    factorGenerate: vi.fn(),
    factorDelete: vi.fn(),
    factorImportOtpauth: vi.fn(),
    factorAddExternal: vi.fn(),
    recoveryList: vi.fn(),
    recoveryMarkUsed: vi.fn(),
    recoveryDelete: vi.fn(),
    recoveryAdd: vi.fn(),
  },
}));

const account: Account = {
  id: "acc-1",
  profileId: "prof-1",
  displayName: "Work GitHub",
  serviceKind: { kind: "github" },
  username: "octocat",
  email: "octo@example.com",
  loginUrl: "https://github.com/login",
  tags: ["work"],
  notes: "",
  passwordRef: "ref-1",
  factors: [],
  recoveryCodes: [],
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

const noop = () => Promise.resolve();

beforeEach(async () => {
  await initI18n("en-US");
  vi.mocked(api.passwordReveal).mockResolvedValue("s3cret-value");
});

function renderCard(locked = false) {
  return render(
    <AccountCard
      account={account}
      locked={locked}
      onChanged={noop}
      onEdit={() => {}}
      onDelete={() => {}}
      onError={() => {}}
      onToast={() => {}}
    />,
  );
}

describe("AccountCard", () => {
  it("shows identity and action affordances", () => {
    renderCard();
    expect(screen.getByText("Work GitHub")).toBeDefined();
    expect(screen.getByText("GitHub")).toBeDefined();
    expect(screen.getByRole("button", { name: "Copy" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Reveal" })).toBeDefined();
  });

  it("reveals a password only after an explicit action", async () => {
    renderCard();
    expect(screen.queryByText("s3cret-value")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Reveal" }));
    await waitFor(() => expect(screen.getByText("s3cret-value")).toBeDefined());
  });

  it("removes revealed secret state the moment the vault locks", async () => {
    const { rerender } = renderCard();
    fireEvent.click(screen.getByRole("button", { name: "Reveal" }));
    await waitFor(() => expect(screen.getByText("s3cret-value")).toBeDefined());

    rerender(
      <AccountCard
        account={account}
        locked={true}
        onChanged={noop}
        onEdit={() => {}}
        onDelete={() => {}}
        onError={() => {}}
        onToast={() => {}}
      />,
    );
    expect(screen.queryByText("s3cret-value")).toBeNull();
    expect(screen.getByRole("button", { name: "Copy" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByRole("button", { name: "Reveal" }).hasAttribute("disabled")).toBe(true);
  });

  it("hides secret actions entirely when no password is stored", () => {
    render(
      <AccountCard
        account={{ ...account, passwordRef: null }}
        locked={false}
        onChanged={noop}
        onEdit={() => {}}
        onDelete={() => {}}
        onError={() => {}}
        onToast={() => {}}
      />,
    );
    expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Reveal" })).toBeNull();
    expect(screen.getByText("No password")).toBeDefined();
  });
});
