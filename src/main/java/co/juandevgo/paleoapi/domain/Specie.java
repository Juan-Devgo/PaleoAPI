package co.juandevgo.paleoapi.domain;

import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NonNull;

import java.util.List;
import java.util.UUID;

@Data
@Builder
@AllArgsConstructor
public class Specie {

    private UUID id;

    @NonNull
    private String name;

    @NonNull
    private String group;

    private String phonetic;

    @NonNull
    private String diet;

    @NonNull
    private String size;

    @NonNull
    private String timePeriod;

    @NonNull
    private List<String> locations;

    @NonNull
    private List<String> classification;

    private List<String> images;
}