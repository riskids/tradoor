import { assert } from 'jsr:@std/assert';
import { localIsOurs } from './store.ts';

Deno.test('a layout kept in this browser is only opened with when it is this browser’s', () => {
  assert(localIsOurs('abc123', 'abc123'), 'the copy this browser saved');
  assert(localIsOurs(null, 'abc123'), 'a copy from before the stamp existed is this browser’s own');
  assert(
    !localIsOurs('abc123', 'def456'),
    'pasting another browser’s id asks for their layout: opening with this one would save it over theirs',
  );
});
