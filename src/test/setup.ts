import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// jsdom 尚未实现原生 dialog 的生命周期方法。
HTMLDialogElement.prototype.showModal ??= function () { this.open = true; };
HTMLDialogElement.prototype.close ??= function () { this.open = false; };

// vitest 未开 globals 时 testing-library 不会自动清理，
// 同一文件里的多次 render 会叠加在同一个 document 上
afterEach(cleanup);
