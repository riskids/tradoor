/// <reference lib="deno.ns" />
/// <reference lib="dom" />
import { assertEquals } from 'jsr:@std/assert@1';
import { magnet, setMagnet, setMagnetHeld, toggleMagnet } from './ui.ts';

Deno.test('the magnet turns on in the mode last picked, and Ctrl or ⌘ held turns it the other way round', () => {
  // Deno keeps localStorage between runs: start from weak and off, whatever the last run left.
  setMagnet('weak');
  setMagnet('off');
  assertEquals(magnet(), 'off');
  setMagnetHeld(true);
  assertEquals(magnet(), 'weak', 'held over no magnet: on, weak until strong is picked');
  setMagnetHeld(false);
  assertEquals(magnet(), 'off', 'let go: off again');
  setMagnet('strong');
  setMagnetHeld(true);
  assertEquals(magnet(), 'off', 'held over a magnet: off');
  setMagnetHeld(false);
  assertEquals(magnet(), 'strong');
  toggleMagnet();
  assertEquals(magnet(), 'off');
  toggleMagnet();
  assertEquals(magnet(), 'strong', 'on again as it was last picked');
});
