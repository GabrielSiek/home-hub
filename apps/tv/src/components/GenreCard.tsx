import { genreStyle, type Genre } from '../lib/genres';
import { FocusButton } from './FocusButton';

interface Props {
  genre: Genre;
  focusKey?: string;
  onFocus?: () => void;
  onSelect: () => void;
}

/** A genre's photo tinted in its color, with a color bar over its name. */
export function GenreCard({ genre, focusKey, onFocus, onSelect }: Props) {
  return (
    <FocusButton
      focusKey={focusKey}
      className="genre-card"
      style={genreStyle(genre)}
      aria-label={genre.name}
      onFocus={onFocus}
      onSelect={onSelect}
    >
      <img className="genre-card-img" src={genre.image} alt="" />
      <span className="genre-card-tint" aria-hidden="true" />
      <span className="genre-card-wash" aria-hidden="true" />
      <span className="genre-card-edge" aria-hidden="true" />
      <span className="genre-card-text">
        <span className="genre-bar" aria-hidden="true" />
        <span className="genre-card-name">{genre.name}</span>
      </span>
    </FocusButton>
  );
}
