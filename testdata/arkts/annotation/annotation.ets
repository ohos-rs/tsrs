import { Retention, RetentionPolicy } from './@arkts.lang';
@Retention({ policy: RetentionPolicy.SOURCE })
export @interface Source {
  value = 42;
}
@Source
export interface I {
  @Source value: number;
}
@Source
export function f(): number {
  return 1;
}
@Source
export class C {
  @Source method() {}
}
