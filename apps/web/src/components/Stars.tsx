/** Zero to three stars, as earned in Learn. */
export function Stars({ count, small = false }: { count: number; small?: boolean }) {
  return (
    <span className={`stars ${small ? 'small' : ''}`} role="img" aria-label={`${count} of 3 stars`}>
      {[0, 1, 2].map((i) => (
        <span key={i} className={`star ${i < count ? 'on' : ''}`} aria-hidden>
          ★
        </span>
      ))}
    </span>
  );
}
