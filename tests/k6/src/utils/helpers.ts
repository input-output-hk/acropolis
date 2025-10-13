import { sleep } from 'k6';
import { EndpointWeight } from '../types';

export function weightedRandomChoice(choices: EndpointWeight[]): () => void {
  const totalWeight = choices.reduce((sum, c) => sum + c.weight, 0);
  let random = Math.random() * totalWeight;

  for (const choice of choices) {
    random -= choice.weight;
    if (random <= 0) return choice.fn;
  }

  return choices[0].fn;
}

export function randomSleep(min: number = 1, max: number = 3): void {
  sleep(Math.random() * (max - min) + min);
}